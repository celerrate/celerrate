//! `--output=sarif`: SARIF 2.1.0 over the same final stream as the human
//! renderer and the JSON writer.
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
fn sarif_validates_against_the_official_schema() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/sarif-2.1.0.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let findings = findings_project();
    let clean = project(&[
        ("composer.json", MANIFEST),
        ("src/Clean.php", CLEAN_EXAMPLE),
    ]);
    let noticed = project(&[("src/Example.php", FAILING_EXAMPLE)]);
    for root in [findings.path(), clean.path(), noticed.path()] {
        let (_, text) = check_with(root, &["--output", "sarif"]);
        let instance: serde_json::Value = serde_json::from_str(&text).unwrap();
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
}

#[test]
fn results_carry_locations_and_referenced_rules_are_described() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "sarif"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let run = &value["runs"][0];
    assert_eq!(run["columnKind"], "unicodeCodePoints");
    let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
    let described: Vec<&str> = rules
        .iter()
        .map(|rule| rule["id"].as_str().unwrap())
        .collect();
    let results = run["results"].as_array().unwrap();
    assert!(!results.is_empty());
    for result in results {
        let rule_id = result["ruleId"].as_str().unwrap();
        assert!(described.contains(&rule_id), "{rule_id} lacks a descriptor");
        if result["level"] != "note" {
            let region = &result["locations"][0]["physicalLocation"]["region"];
            assert!(region["startLine"].as_u64().unwrap() >= 1);
        }
    }
}

#[test]
fn notices_become_note_level_results_without_location() {
    let root = project(&[("src/Example.php", FAILING_EXAMPLE)]);
    let (_, text) = check_with(root.path(), &["--output", "sarif"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let results = value["runs"][0]["results"].as_array().unwrap();
    let notes: Vec<_> = results.iter().filter(|r| r["level"] == "note").collect();
    assert!(!notes.is_empty());
    for note in notes {
        assert!(note.get("locations").is_none());
    }
}

#[test]
fn the_invocation_embeds_the_exit_code() {
    let root = findings_project();
    let (outcome, text) = check_with(root.path(), &["--output", "sarif"]);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let invocation = &value["runs"][0]["invocations"][0];
    assert_eq!(
        invocation["exitCode"].as_u64().unwrap(),
        u64::from(outcome.code())
    );
    assert_eq!(invocation["executionSuccessful"], true);
}

/// An internal error the run survives must appear as a
/// `toolExecutionNotification` on the invocation, not a result: it is a
/// problem the tool itself hit, not a finding about the analyzed code.
#[cfg(unix)]
#[test]
fn internal_errors_become_tool_execution_notifications() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = project(&[
        ("src/Kernel.php", "<?php\nclass Kernel {}\n"),
        ("src/locked/Hidden.php", "<?php\nclass Hidden {}\n"),
    ]);
    let locked = root.path().join("src/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let (outcome, text) = check_with(root.path(), &["--output", "sarif"]);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(outcome, Outcome::InternalError, "{text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let invocation = &value["runs"][0]["invocations"][0];
    assert_eq!(invocation["executionSuccessful"], false);
    let notifications = invocation["toolExecutionNotifications"]
        .as_array()
        .unwrap_or_else(|| panic!("expected toolExecutionNotifications: {text}"));
    assert_eq!(notifications.len(), 1, "{text}");
    assert_eq!(notifications[0]["level"], "error");
    assert!(
        notifications[0]["message"]["text"]
            .as_str()
            .unwrap()
            .contains("could not be read"),
        "{text}",
    );
    assert_eq!(notifications[0]["descriptor"]["id"], "directory-unreadable");
}

#[test]
fn sarif_findings_snapshot() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "sarif"]);
    insta::assert_snapshot!("sarif_findings", text);
}
