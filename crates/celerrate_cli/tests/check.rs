//! The product, end to end. `run` takes its arguments and its output as
//! values, so these drive the whole thing in process: no spawning, no
//! timing flakiness, and the rendering pinned exactly.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{Outcome, run};

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

fn check(root: &Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()],
        &mut output,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn a_clean_project_reports_nothing_and_exits_zero() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        ("src/Kernel.php", "<?php\nnamespace App;\nclass Kernel {}\n"),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean);
    insta::assert_snapshot!("clean", text);
}

#[test]
fn a_project_with_findings_renders_notices_diagnostics_and_a_summary() {
    // Zero configuration: no manifest, no PHP version. Both fall back,
    // both say so, and neither blocks.
    let root = project(&[(
        "src/Kernel.php",
        "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    insta::assert_snapshot!("findings", text);
}

#[test]
fn notices_alone_are_not_a_failure() {
    // Every notice announces a fallback already taken. Zero-configuration
    // must never block, so notices never touch the exit code.
    let root = project(&[("src/Kernel.php", "<?php\nnamespace App;\nclass Kernel {}\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean);
    assert!(
        text.contains("CEL0025"),
        "the missing manifest is announced"
    );
    assert!(text.contains("0 diagnostics"));
}

#[test]
fn a_warning_alone_still_exits_one() {
    // 1 means "any diagnostic reported", warning or error alike.
    // `utf8_encode` is deprecated since PHP 8.2 (a warning, CEL0023), and
    // the shipped stub blob carries that deprecation: `^8.1` admits the
    // supported range [8.1, 8.5], whose maximum (8.5) is past 8.2, so the
    // deprecation always applies here.
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/Legacy.php",
            "<?php\nnamespace App;\nfunction f(): void { \\utf8_encode('x'); }\n",
        ),
    ]);
    let (outcome, text) = check(root.path());
    assert!(
        text.contains("CEL0023"),
        "the deprecation warning fires: {text}"
    );
    assert_eq!(outcome, Outcome::DiagnosticsReported);
}
