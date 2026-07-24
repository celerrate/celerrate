//! `celerrate.toml` loading: configuration diagnostics are reported
//! span-anchored and affect the exit code; a missing file changes
//! nothing (zero-config parity); the configuration itself is not yet
//! consumed (part 2).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::ffi::OsString;
use std::fs;

use celerrate_cli::{ColorMode, Outcome};

const MANIFEST: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
const CLEAN_SOURCE: &str = "<?php\nnamespace App;\n\nfunction example(): void {}\n";

fn check(files: &[(&str, &str)]) -> (Outcome, String) {
    let directory = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = directory.path().join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full, contents).unwrap();
    }
    run_check(directory.path())
}

/// Like `check`, but for the one case that needs a byte payload no
/// `&str` can carry: a `celerrate.toml` that is not valid UTF-8.
fn check_with_invalid_utf8_configuration(
    files: &[(&str, &str)],
    invalid_utf8: &[u8],
) -> (Outcome, String) {
    let directory = tempfile::tempdir().unwrap();
    for (path, contents) in files {
        let full = directory.path().join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full, contents).unwrap();
    }
    fs::write(directory.path().join("celerrate.toml"), invalid_utf8).unwrap();
    run_check(directory.path())
}

fn run_check(root: &std::path::Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = celerrate_cli::run(
        vec![
            OsString::from("celerrate"),
            OsString::from("check"),
            root.as_os_str().to_owned(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn without_a_configuration_file_a_clean_project_stays_clean() {
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
    ]);
    assert!(matches!(outcome, Outcome::Clean), "report was:\n{report}");
    assert!(!report.contains("CEL004"), "report was:\n{report}");
}

#[test]
fn a_valid_configuration_file_reports_nothing() {
    let configuration = "[project]\nphp = \"8.2\"\n\n[rules.null-dereference]\nenabled = false\n";
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("celerrate.toml", configuration),
    ]);
    assert!(matches!(outcome, Outcome::Clean), "report was:\n{report}");
}

#[test]
fn a_syntax_error_in_the_configuration_exits_one_with_a_rich_block() {
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("celerrate.toml", "[project\n"),
    ]);
    assert!(
        matches!(outcome, Outcome::DiagnosticsReported),
        "report was:\n{report}",
    );
    assert!(report.contains("CEL0043"), "report was:\n{report}");
    assert!(report.contains("celerrate.toml"), "report was:\n{report}");
}

#[test]
fn an_unknown_rule_in_the_configuration_exits_one() {
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        (
            "celerrate.toml",
            "[rules.nul-dereference]\nenabled = false\n",
        ),
    ]);
    assert!(
        matches!(outcome, Outcome::DiagnosticsReported),
        "report was:\n{report}",
    );
    assert!(report.contains("CEL0046"), "report was:\n{report}");
    assert!(report.contains("nul-dereference"), "report was:\n{report}");
}

#[test]
fn a_resilience_remap_in_the_configuration_exits_one() {
    let (outcome, report) = check(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("celerrate.toml", "[severity]\n\"CEL0026\" = \"error\"\n"),
    ]);
    assert!(
        matches!(outcome, Outcome::DiagnosticsReported),
        "report was:\n{report}",
    );
    assert!(report.contains("CEL0049"), "report was:\n{report}");
}

/// A `celerrate.toml` that is not valid UTF-8 is the one path that asks
/// the renderer to excerpt an empty source (`configuration::unreadable`
/// builds a `LoadedConfiguration` with an empty `text`). This proves the
/// full CLI path survives that render rather than panicking: an internal
/// error is exactly what a render panic would produce, so asserting
/// `DiagnosticsReported`, not `InternalError`, is what pins the behavior.
#[test]
fn a_non_utf8_configuration_file_reports_a_diagnostic_and_does_not_crash_the_renderer() {
    let (outcome, report) = check_with_invalid_utf8_configuration(
        &[
            ("composer.json", MANIFEST),
            ("src/Example.php", CLEAN_SOURCE),
        ],
        &[0xff, 0xfe, 0x00],
    );
    assert!(
        matches!(outcome, Outcome::DiagnosticsReported),
        "an unreadable configuration source must still render, not internal-error: report was:\n{report}",
    );
    assert!(report.contains("CEL0043"), "report was:\n{report}");
}
