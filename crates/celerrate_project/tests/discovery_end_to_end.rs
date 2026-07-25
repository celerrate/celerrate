//! The part-level proof: the whole front door — discovery, walk,
//! virtual file system, database — is a pure function of the disk
//! state.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

use celerrate_db::testing::TestDatabase;
use celerrate_db::{SourceFile, file_diagnostics};
use celerrate_project::{FileOrigin, PhpVersion, PhpVersionRange, ProjectNotice, discover};
use celerrate_vfs::{Vfs, enumerate_php_files};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// One analyzed file, reduced to comparable facts: its root-relative
/// path, its origin, and its diagnostic count.
fn analyze(root: &Path) -> Vec<(String, FileOrigin, usize)> {
    let discovery = discover(root, &celerrate_config::Configuration::default());
    assert_eq!(
        discovery.php_version_range,
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
    );
    assert!(discovery.notices.is_empty());
    let walk = enumerate_php_files(&discovery.walk_roots());
    assert!(
        walk.unreadable_directories.is_empty(),
        "every declared root is readable here: {:?}",
        walk.unreadable_directories,
    );
    let mut vfs = Vfs::default();
    let db = TestDatabase::default();
    walk.files
        .iter()
        .map(|path| {
            let file_id = vfs.load_from_disk(path).unwrap();
            let bytes = vfs.contents(file_id).unwrap().to_vec();
            let source = SourceFile::new(&db, file_id, bytes);
            (
                // The comparable identity of a file is its root-relative
                // path. `display` would spell it with the platform's own
                // separator, `src\App.php` on Windows, and the expected
                // values below can pin only one spelling. Joining the
                // components with `/` spells it the same way everywhere.
                path.strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
                discovery.classify(path),
                file_diagnostics(&db, source).len(),
            )
        })
        .collect()
}

#[test]
fn discovery_walk_and_analysis_are_deterministic_end_to_end() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    write(
        &root.join("composer.json"),
        r#"{
            "require": { "php": "^8.1" },
            "autoload": { "psr-4": { "App\\": "src/" } }
        }"#,
    );
    write(&root.join("src/App.php"), "<?php class App {}");
    write(&root.join("src/Broken.php"), "<?php class {");
    write(&root.join("stray.php"), "<?php this file is not declared");
    write(
        &root.join("vendor/composer/installed.json"),
        r#"{
            "packages": [
                {
                    "name": "acme/library",
                    "install-path": "../acme/library",
                    "autoload": { "psr-4": { "Acme\\": "src/" } }
                }
            ]
        }"#,
    );
    write(
        &root.join("vendor/acme/library/src/Library.php"),
        "<?php class Library {}",
    );

    let first = analyze(root);
    let second = analyze(root);
    assert_eq!(first, second, "two from-scratch runs diverged");

    let summary: Vec<(&str, FileOrigin, bool)> = first
        .iter()
        .map(|(name, origin, diagnostics)| (name.as_str(), *origin, *diagnostics > 0))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("src/App.php", FileOrigin::Project, false),
            ("src/Broken.php", FileOrigin::Project, true),
            (
                "vendor/acme/library/src/Library.php",
                FileOrigin::Vendor,
                false,
            ),
        ],
        "walked set, origins, or diagnostics changed: \
         the undeclared stray.php must stay out",
    );
}

#[test]
fn a_manifest_that_cannot_be_read_is_reported_unreadable_not_missing() {
    // A directory sitting where composer.json should be makes the read
    // fail with an IO error that is not not-found. Discovery must name
    // the failure rather than claim the manifest is absent and analyze
    // the wrong file set while telling the user the file is not there.
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    fs::create_dir_all(root.join("composer.json")).unwrap();

    let discovery = discover(root, &celerrate_config::Configuration::default());

    assert!(
        discovery
            .notices
            .iter()
            .any(|notice| matches!(notice, ProjectNotice::UnreadableComposerManifest { .. })),
        "expected an unreadable-manifest notice, got {:?}",
        discovery.notices,
    );
    assert!(
        !discovery
            .notices
            .iter()
            .any(|notice| matches!(notice, ProjectNotice::MissingComposerManifest)),
        "an unreadable manifest must never be reported as missing",
    );
}
