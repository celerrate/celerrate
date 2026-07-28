//! Baseline integration tests: flags, recording, applying, and the
//! spec's three invariants.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::ffi::OsString;
use std::path::Path;

use celerrate_cli::{ColorMode, Outcome};

const MANIFEST: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    root
}

fn check_with(root: &Path, extra: &[&str]) -> (Outcome, String) {
    let mut arguments: Vec<OsString> =
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()];
    arguments.extend(extra.iter().map(Into::into));
    let mut output = Vec::new();
    let outcome = celerrate_cli::run(arguments, &mut output, ColorMode::Plain);
    (outcome, String::from_utf8(output).unwrap())
}

const CLEAN_SOURCE: &str = "<?php\n\nnamespace App;\n\nclass Example\n{\n}\n";

#[test]
fn baseline_with_watch_is_a_usage_error() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
    ]);
    let (outcome, _) = check_with(root.path(), &["--baseline", "--watch"]);
    assert_eq!(outcome, Outcome::UsageError);
}

#[test]
fn baseline_with_fix_is_a_usage_error() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
    ]);
    let (outcome, _) = check_with(root.path(), &["--baseline", "--fix"]);
    assert_eq!(outcome, Outcome::UsageError);
}

#[test]
fn baseline_with_fix_suggestions_is_a_usage_error() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
    ]);
    let (outcome, _) = check_with(root.path(), &["--baseline", "--fix-suggestions"]);
    assert_eq!(outcome, Outcome::UsageError);
}

#[test]
fn baseline_with_ignore_baseline_is_a_usage_error() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
    ]);
    let (outcome, _) = check_with(root.path(), &["--baseline", "--ignore-baseline"]);
    assert_eq!(outcome, Outcome::UsageError);
}

#[test]
fn the_flags_are_accepted_alone_on_a_clean_project() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
    ]);
    let (recorded, _) = check_with(root.path(), &["--baseline"]);
    assert_eq!(recorded, Outcome::Clean);
    let (strict, _) = check_with(root.path(), &["--ignore-baseline"]);
    assert_eq!(strict, Outcome::Clean);
}
