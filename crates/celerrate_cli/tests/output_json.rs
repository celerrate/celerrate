//! `--output=json`: the versioned machine report over the same final
//! stream as the human renderer.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::ffi::OsString;
use std::path::Path;

use celerrate_cli::{ColorMode, Outcome, run};

const MANIFEST: &str = r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#;
const FAILING_EXAMPLE: &str = "<?php\n\nstrlenn(\"hello\");\n";
const CLEAN_EXAMPLE: &str = "<?php\n\n$greeting = \"hello\";\n";

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
    let outcome = run(arguments, &mut output, ColorMode::Plain);
    (outcome, String::from_utf8(output).unwrap())
}

fn findings_project() -> tempfile::TempDir {
    project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", FAILING_EXAMPLE),
    ])
}

#[test]
fn json_reports_schema_version_one_and_the_embedded_exit_code() {
    let root = findings_project();
    let (outcome, text) = check_with(root.path(), &["--output", "json"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["summary"]["exit_code"], 1);
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert!(!diagnostics.is_empty());
    for diagnostic in diagnostics {
        assert!(diagnostic["id"].as_str().unwrap().starts_with("CEL"));
        let anchor = &diagnostic["anchor"];
        if anchor["kind"] == "span" {
            assert!(anchor["start_line"].as_u64().unwrap() >= 1);
            assert!(anchor["start_column"].as_u64().unwrap() >= 1);
            assert!(!anchor["path"].as_str().unwrap().contains('\\'));
        }
    }
}

#[test]
fn a_clean_project_reports_exit_code_zero_and_no_diagnostics() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Clean.php", CLEAN_EXAMPLE),
    ]);
    let (outcome, text) = check_with(root.path(), &["--output", "json"]);
    assert_eq!(outcome, Outcome::Clean);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["summary"]["exit_code"], 0);
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 0);
}

#[test]
fn json_is_the_entire_stdout() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "json"]);
    // One document, nothing before, one trailing newline after.
    assert!(text.starts_with('{'));
    assert!(text.ends_with("}\n"));
    serde_json::from_str::<serde_json::Value>(&text).unwrap();
}

#[test]
fn project_notices_are_carried_and_exit_neutral() {
    // No composer.json: the project notice channel fires.
    let root = project(&[("src/Clean.php", CLEAN_EXAMPLE)]);
    let (_, text) = check_with(root.path(), &["--output", "json"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let notices = value["notices"].as_array().unwrap();
    assert!(!notices.is_empty());
    for notice in notices {
        assert!(notice["id"].as_str().unwrap().starts_with("CEL"));
        assert!(!notice["message"].as_str().unwrap().is_empty());
    }
    assert_eq!(value["summary"]["notices"], notices.len());
}

#[test]
fn an_applied_baseline_hides_findings_from_the_payload() {
    let root = findings_project();
    let (_, _) = check_with(root.path(), &["--baseline"]);
    let (outcome, text) = check_with(root.path(), &["--output", "json"]);
    assert_eq!(outcome, Outcome::Clean);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["summary"]["exit_code"], 0);
    assert!(value["summary"]["baselined_hidden"].as_u64().unwrap() >= 1);
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 0);
}

/// An internal error the run survives (here: a subdirectory nobody may
/// list) must still tell tooling why, not just how many, so a caller
/// that sees exit code 2 does not have to re-run the tool on the human
/// channel to learn what happened.
#[cfg(unix)]
#[test]
fn an_internal_error_carries_its_kind_message_and_bug_flag_in_the_payload() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = project(&[
        ("src/Kernel.php", "<?php\nclass Kernel {}\n"),
        ("src/locked/Hidden.php", "<?php\nclass Hidden {}\n"),
    ]);
    let locked = root.path().join("src/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let (outcome, text) = check_with(root.path(), &["--output", "json"]);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(outcome, Outcome::InternalError, "{text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["summary"]["exit_code"], 2);
    assert_eq!(value["summary"]["internal_errors"], 1);
    let errors = value["internal_errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "{text}");
    assert_eq!(errors[0]["kind"], "directory-unreadable");
    assert!(
        errors[0]["message"]
            .as_str()
            .unwrap()
            .contains("could not be read"),
        "{text}",
    );
    assert_eq!(
        errors[0]["bug"], false,
        "a permissions problem is the environment's condition, not a Celerrate bug: {text}",
    );
}

#[test]
fn machine_output_refuses_the_mutating_and_looping_flags() {
    let root = findings_project();
    for flag in ["--watch", "--fix", "--fix-suggestions", "--baseline"] {
        let (outcome, text) = check_with(root.path(), &["--output", "json", flag]);
        assert_eq!(outcome, Outcome::UsageError, "{flag}");
        assert!(text.contains(flag), "{flag}: {text}");
        assert!(text.contains("--output=json"), "{flag}: {text}");
    }
}

#[test]
fn ignore_baseline_combines_with_machine_output() {
    let root = findings_project();
    let (_, _) = check_with(root.path(), &["--baseline"]);
    let (outcome, text) = check_with(root.path(), &["--output", "json", "--ignore-baseline"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(!value["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn explicit_human_output_is_byte_identical_to_the_default() {
    let root = findings_project();
    let (default_outcome, default_text) = check_with(root.path(), &[]);
    let (human_outcome, human_text) = check_with(root.path(), &["--output", "human"]);
    assert_eq!(default_outcome, human_outcome);
    assert_eq!(default_text, human_text);
}

#[test]
fn json_findings_snapshot() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "json"]);
    insta::assert_snapshot!("json_findings", text);
}

#[test]
fn json_output_validates_against_the_committed_schema() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../schemas/celerrate-json-report.v1.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    // Shapes covered: findings, the same findings with the baseline
    // ignored, a clean project, a project that only reports a notice, and
    // (unix only, see below) a run whose `internal_errors` array is
    // actually populated rather than empty.
    let findings = findings_project();
    let clean = project(&[
        ("composer.json", MANIFEST),
        ("src/Clean.php", CLEAN_EXAMPLE),
    ]);
    let noticed = project(&[("src/Example.php", FAILING_EXAMPLE)]);
    let (_, _) = check_with(findings.path(), &["--baseline"]);
    let mut runs = vec![
        check_with(findings.path(), &["--output", "json"]).1,
        check_with(findings.path(), &["--output", "json", "--ignore-baseline"]).1,
        check_with(clean.path(), &["--output", "json"]).1,
        check_with(noticed.path(), &["--output", "json"]).1,
    ];
    // Every run above reports zero internal errors, so `$defs/internalError`
    // (the `kind`/`message`/`bug` shape and the `kind` pattern) is never
    // exercised by them alone, and the schema places no `minItems` on
    // `internal_errors` that would catch an accidentally-empty array. A
    // permission-locked directory is the fixture that forces a populated
    // `internal_errors` array; building one needs
    // `std::os::unix::fs::PermissionsExt`, which has no portable
    // equivalent, so `internal_error_report_text` below only produces a
    // shape on unix (`None` elsewhere). The `push` call stays unconditional
    // source so `runs` genuinely needs `mut` on every platform, and the
    // assertion below fires whenever the shape is actually present, so the
    // fixture degrading (for instance a root-run CI job for which
    // `chmod 0o000` does not block reads) fails loudly instead of letting
    // this run validate the schema vacuously.
    if let Some(text) = internal_error_report_text() {
        let instance: serde_json::Value = serde_json::from_str(&text).unwrap();
        let errors = instance["internal_errors"].as_array().unwrap();
        assert!(
            !errors.is_empty(),
            "the locked-directory fixture must report at least one internal \
             error, or this shape validates $defs/internalError vacuously: {text}",
        );
        assert_eq!(
            instance["summary"]["internal_errors"].as_u64().unwrap(),
            errors.len() as u64,
            "summary.internal_errors must agree with the internal_errors array length: {text}",
        );
        runs.push(text);
    }
    for text in runs {
        let instance: serde_json::Value = serde_json::from_str(&text).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
}

/// Builds the permission-locked-directory project, runs it through
/// `--output=json`, restores the directory's permissions immediately so
/// the temporary directory can still be cleaned up, and returns the JSON
/// text. `None` on non-unix targets: there is no portable way to build a
/// directory the process cannot read, so this shape is simply unavailable
/// there.
#[cfg(unix)]
fn internal_error_report_text() -> Option<String> {
    use std::os::unix::fs::PermissionsExt as _;

    let root = project(&[
        ("src/Kernel.php", "<?php\nclass Kernel {}\n"),
        ("src/locked/Hidden.php", "<?php\nclass Hidden {}\n"),
    ]);
    let locked = root.path().join("src/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let text = check_with(root.path(), &["--output", "json"]).1;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    Some(text)
}

#[cfg(not(unix))]
fn internal_error_report_text() -> Option<String> {
    None
}
