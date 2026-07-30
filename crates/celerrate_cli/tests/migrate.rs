//! `celerrate migrate --from-phpstan` end to end: conversion, the
//! report, the clean-slate baseline, and the continuity contract.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::ffi::OsString;
use std::path::Path;

use celerrate_cli::{ColorMode, Outcome, run};

const MANIFEST: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
const FAILING_EXAMPLE: &str = "<?php\n\nstrlenn(\"hello\");\n";
const SUPPRESSED_EXAMPLE: &str = "<?php\n\nstrlenn(\"hello\"); // @phpstan-ignore-line\n";
const CLEAN_EXAMPLE: &str = "<?php\n\n$greeting = \"hello\";\n";

// Tab-indented, as phpstan.neon conventionally is: a recursive
// include, a scattered baseline include, level 5, and keys that do
// not carry over.
const PHPSTAN_NEON: &str = "includes:\n\t- phpstan-baseline.neon\n\t- build/strict.neon\n\nparameters:\n\tlevel: 5\n\tpaths:\n\t\t- src\n\texcludePaths:\n\t\t- src/Generated\n\tignoreErrors:\n\t\t-\n\t\t\tmessage: '#unused#'\n\t\t\tpath: src/Legacy.php\n\tbootstrapFiles:\n\t\t- tests/bootstrap.php\n";
const STRICT_NEON: &str = "parameters:\n\tlevel: 3\n\texcludePaths:\n\t\t- fixtures\n";
const BASELINE_NEON: &str = "parameters:\n\tignoreErrors: []\n";

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

fn migrate_with(root: &Path, extra: &[&str]) -> (Outcome, String) {
    let mut arguments: Vec<OsString> = vec![
        "celerrate".into(),
        "migrate".into(),
        root.as_os_str().into(),
        "--from-phpstan".into(),
    ];
    arguments.extend(extra.iter().map(Into::into));
    let mut output = Vec::new();
    let outcome = run(arguments, &mut output, ColorMode::Plain);
    (outcome, String::from_utf8(output).unwrap())
}

fn migrate(root: &Path) -> (Outcome, String) {
    migrate_with(root, &[])
}

fn check(root: &Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()],
        &mut output,
        ColorMode::Plain,
    );
    (outcome, String::from_utf8(output).unwrap())
}

fn phpstan_project() -> tempfile::TempDir {
    project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", FAILING_EXAMPLE),
        ("src/Suppressed.php", SUPPRESSED_EXAMPLE),
        ("phpstan.neon", PHPSTAN_NEON),
        ("build/strict.neon", STRICT_NEON),
        ("phpstan-baseline.neon", BASELINE_NEON),
    ])
}

#[test]
fn migrate_converts_reports_and_records_the_clean_slate() {
    let root = phpstan_project();
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");

    // The generated configuration.
    let configuration = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    assert!(
        configuration.contains("include = [\"src\"]"),
        "file was:\n{configuration}"
    );
    assert!(
        configuration.contains("exclude = [\"build/fixtures\", \"src/Generated\"]"),
        "file was:\n{configuration}"
    );
    assert!(
        configuration.contains("CEL0034 = \"warning\""),
        "file was:\n{configuration}"
    );

    // The report: level, untransposed keys, the ignored baseline.
    assert!(text.contains("level 5"), "report was:\n{text}");
    assert!(text.contains("ignoreErrors"), "report was:\n{text}");
    assert!(text.contains("bootstrapFiles"), "report was:\n{text}");
    assert!(
        text.contains("phpstan-baseline.neon"),
        "report was:\n{text}"
    );

    // The clean slate: the finding is recorded, the suppressed one is
    // not (suppression is in-engine, upstream of recording).
    let baseline = std::fs::read_to_string(root.path().join("celerrate-baseline.toml")).unwrap();
    assert!(
        baseline.contains("path = \"src/Example.php\""),
        "file was:\n{baseline}"
    );
    assert!(!baseline.contains("Suppressed"), "file was:\n{baseline}");
    assert!(
        text.contains("recorded 1 baseline entry"),
        "report was:\n{text}"
    );
}

#[test]
fn migrate_never_touches_the_phpstan_files() {
    let root = phpstan_project();
    migrate(root.path());
    let neon = std::fs::read_to_string(root.path().join("phpstan.neon")).unwrap();
    assert_eq!(neon, PHPSTAN_NEON);
    let baseline = std::fs::read_to_string(root.path().join("phpstan-baseline.neon")).unwrap();
    assert_eq!(baseline, BASELINE_NEON);
}

#[test]
fn after_migrate_the_first_check_is_clean_and_only_new_problems_fail() {
    let root = phpstan_project();
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");

    let (first, text) = check(root.path());
    assert_eq!(first, Outcome::Clean, "report was:\n{text}");

    std::fs::write(
        root.path().join("src/Fresh.php"),
        "<?php\n\nstrlenn(\"fresh\");\n",
    )
    .unwrap();
    let (second, text) = check(root.path());
    assert_eq!(second, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("Fresh.php"), "report was:\n{text}");
}

#[test]
fn a_clean_project_records_no_baseline() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Clean.php", CLEAN_EXAMPLE),
        (
            "phpstan.neon",
            "parameters:\n\tlevel: 8\n\tpaths:\n\t\t- src\n",
        ),
    ]);
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(!root.path().join("celerrate-baseline.toml").exists());
    assert!(text.contains("no findings"), "report was:\n{text}");
    // Level 8 keeps defaults: no severity section.
    let configuration = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    assert!(
        !configuration.contains("[severity]"),
        "file was:\n{configuration}"
    );
}

#[test]
fn a_dist_source_is_discovered() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Clean.php", CLEAN_EXAMPLE),
        (
            "phpstan.neon.dist",
            "parameters:\n\tlevel: 6\n\tpaths:\n\t\t- src\n",
        ),
    ]);
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("phpstan.neon.dist"), "report was:\n{text}");
}

#[test]
fn without_a_phpstan_configuration_migrate_is_a_usage_error() {
    let root = project(&[("composer.json", MANIFEST)]);
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::UsageError, "report was:\n{text}");
    assert!(text.contains("phpstan.neon"), "report was:\n{text}");
    assert!(!root.path().join("celerrate.toml").exists());
}

#[test]
fn an_existing_configuration_is_refused_without_force() {
    let root = phpstan_project();
    std::fs::write(root.path().join("celerrate.toml"), "# hand-written\n").unwrap();
    let (outcome, text) = migrate(root.path());
    assert_eq!(outcome, Outcome::UsageError, "report was:\n{text}");
    assert!(text.contains("--force"), "report was:\n{text}");
    let untouched = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    assert_eq!(untouched, "# hand-written\n");
}

#[test]
fn force_overwrites_deterministically() {
    let root = phpstan_project();
    let (first_outcome, _) = migrate(root.path());
    assert_eq!(first_outcome, Outcome::Clean);
    let first = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    let (second_outcome, text) = migrate_with(root.path(), &["--force"]);
    assert_eq!(second_outcome, Outcome::Clean, "report was:\n{text}");
    let second = std::fs::read_to_string(root.path().join("celerrate.toml")).unwrap();
    assert_eq!(first, second);
}

#[test]
fn migrate_without_a_source_flag_is_a_usage_error() {
    let root = phpstan_project();
    let mut output = Vec::new();
    let outcome = run(
        vec![
            "celerrate".into(),
            "migrate".into(),
            root.path().as_os_str().into(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    let text = String::from_utf8(output).unwrap();
    assert_eq!(outcome, Outcome::UsageError, "report was:\n{text}");
    assert!(text.contains("--from-phpstan"), "report was:\n{text}");
}

#[test]
fn an_unusable_root_is_a_usage_error() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("vanished");
    let (outcome, text) = migrate(&missing);
    assert_eq!(outcome, Outcome::UsageError, "report was:\n{text}");
}

#[test]
fn the_migration_report_snapshot() {
    let root = phpstan_project();
    let (_, text) = migrate(root.path());
    insta::assert_snapshot!("migrate_report", text);
}
