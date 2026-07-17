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

/// The fetched pinned snapshot, if present: reads the pin file
/// committed next to xtask and points into `target/`. `None` when the
/// snapshot has not been fetched (or the pin is unreadable): callers
/// treat that as "nothing to compare against", never as an error.
pub fn pinned_snapshot_directory() -> Option<PathBuf> {
    let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_directory.parent()?.parent()?;
    let pin_text = std::fs::read_to_string(workspace_root.join("xtask/phpstorm-stubs.pin")).ok()?;
    let commit = pin_text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "commit").then(|| value.trim().to_owned())
    })?;
    let directory = workspace_root.join("target/phpstorm-stubs").join(commit);
    directory.is_dir().then_some(directory)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic)]

    use std::fs;
    use std::path::Path;

    use crate::blob::{encode, fnv1a64};
    use crate::compiler::extract::extract;
    use crate::compiler::refinement_source::{parse_refinement_source, validate_refinements};
    use crate::index::StubIndex;

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

    /// The committed blob must be exactly what the pinned snapshot
    /// compiles to. Runs only when the snapshot has been fetched
    /// (`cargo xtask fetch-stubs`); CI enforces it unconditionally
    /// through `cargo xtask compile-stubs --check`. Debug-build note:
    /// this parses the whole snapshot and can take a few minutes.
    #[test]
    fn the_committed_blob_matches_a_recompilation_of_the_pinned_snapshot() {
        let Some(snapshot) = super::pinned_snapshot_directory() else {
            eprintln!(
                "skipped: pinned snapshot not fetched; run `cargo xtask fetch-stubs` to enable this test",
            );
            return;
        };
        let files = match super::stub_files(&snapshot) {
            Ok(files) => files,
            Err(error) => panic!("cannot walk the snapshot: {error}"),
        };
        let mut symbols = Vec::new();
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        for path in &files {
            if let Ok(text) = std::fs::read_to_string(path) {
                let extraction = extract(&text);
                symbols.extend(extraction.symbols);
                functions.extend(extraction.functions);
                classes.extend(extraction.classes);
            }
        }
        let mut index = StubIndex::new(symbols, functions, classes);

        // `stub-compiler` always attaches the committed refinements
        // overlay too (`xtask/src/stubs.rs`'s `--refinements` flag),
        // so a faithful recompilation must parse and apply it the same
        // way, or this freshness check would flag every healthy commit
        // as stale.
        let refinements_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("refinements.celerrate");
        let refinements_text = match fs::read_to_string(&refinements_path) {
            Ok(text) => text,
            Err(error) => panic!("cannot read {}: {error}", refinements_path.display()),
        };
        let refinements = match parse_refinement_source(&refinements_text) {
            Ok(refinements) => refinements,
            Err(error) => panic!("{}: {error}", refinements_path.display()),
        };
        if let Err(error) = validate_refinements(&refinements, index.functions(), index.classes()) {
            panic!("{}: {error}", refinements_path.display());
        }
        index.set_refinements(refinements);

        let recompiled = encode(&index);
        let committed = crate::EMBEDDED_STUB_BLOB;
        // Compare via length + hash: a byte-for-byte assert_eq would
        // dump megabytes on failure.
        assert!(
            recompiled.len() == committed.len() && fnv1a64(&recompiled) == fnv1a64(committed),
            "src/stubs.bin is stale: run `cargo xtask compile-stubs` and commit the result",
        );
    }
}
