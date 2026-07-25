//! `celerrate.toml` loading: configuration diagnostics are reported
//! span-anchored and affect the exit code; a missing file changes
//! nothing (zero-config parity); `include`, `exclude`, and the `php`
//! override are consumed by discovery and the walk.

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
    write_files(directory.path(), files);
    run_check(directory.path())
}

/// Like `check`, but for the one case that needs a byte payload no
/// `&str` can carry: a `celerrate.toml` that is not valid UTF-8.
fn check_with_invalid_utf8_configuration(
    files: &[(&str, &str)],
    invalid_utf8: &[u8],
) -> (Outcome, String) {
    let directory = tempfile::tempdir().unwrap();
    write_files(directory.path(), files);
    fs::write(directory.path().join("celerrate.toml"), invalid_utf8).unwrap();
    run_check(directory.path())
}

/// Writes each `(path, contents)` pair under `root`, creating parent
/// directories as needed. Shared by `check` and
/// `check_with_invalid_utf8_configuration`.
fn write_files(root: &std::path::Path, files: &[(&str, &str)]) {
    for (path, contents) in files {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full, contents).unwrap();
    }
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

#[test]
fn include_widens_the_analysis_to_directories_composer_does_not_declare() {
    let files: &[(&str, &str)] = &[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        ("scripts/tool.php", "<?php\nnew MissingFromScripts();\n"),
    ];
    let (outcome, report) = check(files);
    assert!(
        matches!(outcome, Outcome::Clean),
        "without include, scripts/ is not walked: {report}",
    );

    let mut with_include = files.to_vec();
    with_include.push((
        "celerrate.toml",
        "[project]\ninclude = [\"src\", \"scripts\"]\n",
    ));
    let (outcome, report) = check(&with_include);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("CEL0018"), "{report}");
    assert!(report.contains("MissingFromScripts"), "{report}");
}

#[test]
fn exclude_subtracts_a_directory_from_the_analysis() {
    let files: &[(&str, &str)] = &[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        (
            "src/Generated/Machine.php",
            "<?php\nnew MissingFromGenerated();\n",
        ),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");

    let mut with_exclude = files.to_vec();
    with_exclude.push((
        "celerrate.toml",
        "[project]\nexclude = [\"src/Generated\"]\n",
    ));
    let (outcome, report) = check(&with_exclude);
    assert!(
        matches!(outcome, Outcome::Clean),
        "the excluded directory no longer speaks: {report}",
    );
}

#[test]
fn the_php_override_collapses_the_range_and_gates_availability() {
    let files: &[(&str, &str)] = &[
        ("composer.json", r#"{"require": {"php": ">=8.1"}}"#),
        ("a.php", "<?php json_validate('{}');\n"),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("CEL0021"), "{report}");

    let mut with_override = files.to_vec();
    with_override.push(("celerrate.toml", "[project]\nphp = \"8.3\"\n"));
    let (outcome, report) = check(&with_override);
    assert!(
        matches!(outcome, Outcome::Clean),
        "at a fixed 8.3 the symbol exists: {report}",
    );
}

const NULLABLE_MANIFEST: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
const NULLABLE_SOURCE: &str = "<?php\nnamespace App;\n\nclass User { public function save(): void {} }\n\nclass Consumer\n{\n    public function run(?User $maybe): void\n    {\n        $maybe->save();\n    }\n}\n";

#[test]
fn disabling_a_default_rule_removes_its_diagnostics() {
    let files: &[(&str, &str)] = &[
        ("composer.json", NULLABLE_MANIFEST),
        ("src/Consumer.php", NULLABLE_SOURCE),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("CEL0034"), "{report}");

    let mut disabled = files.to_vec();
    disabled.push((
        "celerrate.toml",
        "[rules.null-dereference]\nenabled = false\n",
    ));
    let (outcome, report) = check(&disabled);
    assert!(matches!(outcome, Outcome::Clean), "{report}");
}

#[test]
fn enabling_a_default_rule_is_a_silent_no_op() {
    let (outcome, report) = check(&[
        ("composer.json", NULLABLE_MANIFEST),
        ("src/Consumer.php", NULLABLE_SOURCE),
        (
            "celerrate.toml",
            "[rules.null-dereference]\nenabled = true\n",
        ),
    ]);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("CEL0034"), "{report}");
}

#[test]
fn a_severity_remap_changes_the_printed_severity_but_not_the_exit_code() {
    let files: &[(&str, &str)] = &[
        ("composer.json", MANIFEST),
        (
            "src/Example.php",
            "<?php\nnamespace App;\n\nnew \\MissingDependency();\n",
        ),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("error[CEL0018]"), "{report}");

    let mut remapped = files.to_vec();
    remapped.push(("celerrate.toml", "[severity]\n\"CEL0018\" = \"warning\"\n"));
    let (outcome, report) = check(&remapped);
    assert!(
        matches!(outcome, Outcome::DiagnosticsReported),
        "a warning still exits 1: {report}",
    );
    assert!(report.contains("warning[CEL0018]"), "{report}");
    assert!(!report.contains("error[CEL0018]"), "{report}");
}

/// CEL0042 (`unused-suppression`) is produced by the rule framework's
/// fourth, reporting phase, not by the syntax/semantic/typed families
/// `a_severity_remap_changes_the_printed_severity_but_not_the_exit_code`
/// exercises above. The remap must reach it too.
#[test]
fn a_severity_remap_reaches_the_reporting_phase_diagnostics_too() {
    let files: &[(&str, &str)] = &[
        ("composer.json", MANIFEST),
        (
            "src/Example.php",
            "<?php\nnamespace App;\n\n// @celerrate-ignore CEL0030\nfunction example(): void {}\n",
        ),
    ];
    let (outcome, report) = check(files);
    assert!(matches!(outcome, Outcome::DiagnosticsReported), "{report}");
    assert!(report.contains("warning[CEL0042]"), "{report}");

    let mut remapped = files.to_vec();
    remapped.push(("celerrate.toml", "[severity]\n\"CEL0042\" = \"error\"\n"));
    let (outcome, report) = check(&remapped);
    assert!(
        matches!(outcome, Outcome::DiagnosticsReported),
        "an error still exits 1: {report}",
    );
    assert!(report.contains("error[CEL0042]"), "{report}");
    assert!(!report.contains("warning[CEL0042]"), "{report}");
}
