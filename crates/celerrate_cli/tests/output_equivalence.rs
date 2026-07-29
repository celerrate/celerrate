//! Cross-format guarantees: the JSON, SARIF and GitHub writers, and the
//! human renderer, all serialize the same final diagnostic stream. This
//! suite pins that the three machine formats agree with each other and
//! with the human report on findings, order and outcome, that the machine
//! formats are deterministic across repeated runs, and that none of them
//! is affected by the color mode. It adds no production code: a failure
//! here means a writer disagrees with the others, not that an assertion
//! needs loosening.
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

/// `CEL` followed by four digits, in order of appearance.
fn identifier_regex_matches(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 7 <= bytes.len() {
        if &bytes[i..i + 3] == b"CEL" && bytes[i + 3..i + 7].iter().all(u8::is_ascii_digit) {
            found.push(text[i..i + 7].to_owned());
            i += 7;
        } else {
            i += 1;
        }
    }
    found
}

fn json_diagnostic_ids(text: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_owned())
        .collect()
}

/// Only `results` entries, filtered to drop notices: SARIF prepends
/// notices as `note`-level results ahead of the findings, and neither the
/// run's own `properties` (the baselined-hidden count) nor a rule
/// descriptor's `properties.rule` lives inside `results`, so this filter
/// only ever sees findings.
fn sarif_finding_ids(text: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(text).unwrap();
    value["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["level"] != "note")
        .map(|r| r["ruleId"].as_str().unwrap().to_owned())
        .collect()
}

fn github_finding_ids(text: &str) -> Vec<String> {
    // One identifier per annotation line: the leading `CELxxxx: ` token.
    // A message could itself mention an identifier, so only the first
    // match on each line counts.
    text.lines()
        .filter(|line| line.starts_with("::error") || line.starts_with("::warning"))
        .filter_map(|line| identifier_regex_matches(line).into_iter().next())
        .collect()
}

#[test]
fn every_format_reports_the_same_findings_in_the_same_order() {
    let root = findings_project();
    let (_, json) = check_with(root.path(), &["--output", "json"]);
    let (_, sarif) = check_with(root.path(), &["--output", "sarif"]);
    let (_, github) = check_with(root.path(), &["--output", "github"]);
    let (_, human) = check_with(root.path(), &[]);
    let ids = json_diagnostic_ids(&json);
    assert!(!ids.is_empty());
    assert_eq!(ids, sarif_finding_ids(&sarif));
    assert_eq!(ids, github_finding_ids(&github));
    for id in &ids {
        assert!(human.contains(id), "{id} missing from the human report");
    }
}

/// Two findings in one file, positioned so the order the repository
/// actually reports them in disagrees with their identifier order: the
/// unknown-function call on line 3 reports `CEL0019`, and the reference
/// to a missing base class on line 5 reports `CEL0018`. Diagnostics are
/// sorted by position, so the correct order is `CEL0019` then `CEL0018`,
/// the reverse of ascending identifier order. A writer that resorted by
/// identifier, by rule name, or by any order other than the one it was
/// handed would flip this pair.
fn order_sensitive_project() -> tempfile::TempDir {
    project(&[
        ("composer.json", MANIFEST),
        (
            "src/Example.php",
            "<?php\n\nstrlenn(\"hello\");\n\nclass Kernel extends Missing\n{\n}\n",
        ),
    ])
}

/// `every_format_reports_the_same_findings_in_the_same_order` above uses
/// `findings_project()`, which reports exactly one diagnostic: a
/// single-element list trivially matches any other single-element list
/// in the same order, so that test cannot tell a writer that preserves
/// order from one that resorts by severity, by identifier, or by
/// anything else. This test makes the order clause load-bearing: the
/// fixture produces two findings whose position order is the reverse of
/// their identifier order, and the length guard makes sure the fixture
/// degrading back to one finding fails loudly rather than quietly
/// restoring that blind spot.
#[test]
fn every_format_preserves_an_order_that_disagrees_with_identifier_order() {
    let root = order_sensitive_project();
    let (_, json) = check_with(root.path(), &["--output", "json"]);
    let (_, sarif) = check_with(root.path(), &["--output", "sarif"]);
    let (_, github) = check_with(root.path(), &["--output", "github"]);
    let ids = json_diagnostic_ids(&json);
    assert!(
        ids.len() >= 2,
        "the fixture must produce at least two findings, or this test \
         degrades back to the single-finding blind spot: {ids:?}",
    );
    // Pinned to the exact expected order, not just to the three writers
    // agreeing with each other: three writers sharing the same wrong
    // sort would still agree with each other while all disagreeing with
    // the real position order.
    assert_eq!(ids, vec!["CEL0019".to_owned(), "CEL0018".to_owned()]);
    assert_eq!(ids, sarif_finding_ids(&sarif));
    assert_eq!(ids, github_finding_ids(&github));
}

#[test]
fn every_format_agrees_on_the_outcome() {
    let root = findings_project();
    let mut outcomes = Vec::new();
    for format in ["human", "json", "sarif", "github"] {
        outcomes.push(check_with(root.path(), &["--output", format]).0);
    }
    assert!(
        outcomes.windows(2).all(|pair| pair[0] == pair[1]),
        "{outcomes:?}"
    );
}

/// The determinism assertion below runs the same project twice through
/// each format. The first run builds a cold analysis database from
/// scratch; the second run reuses the process-local caches the first run
/// populated, so it takes the warm path. Byte-equality between the two
/// therefore proves not only that repeated runs agree, but that the warm
/// and cold analysis paths serialize to the exact same machine output.
#[test]
fn machine_output_is_deterministic_across_runs() {
    let root = findings_project();
    for format in ["json", "sarif", "github"] {
        let (_, first) = check_with(root.path(), &["--output", format]);
        let (_, second) = check_with(root.path(), &["--output", format]);
        assert_eq!(first, second, "{format}");
    }
}

#[test]
fn machine_output_ignores_the_color_mode() {
    let root = findings_project();
    let colored = celerrate_cli::color_mode(true, None);
    for format in ["json", "sarif", "github"] {
        let mut plain_buffer = Vec::new();
        let mut colored_buffer = Vec::new();
        let arguments = |root: &Path| -> Vec<OsString> {
            vec![
                "celerrate".into(),
                "check".into(),
                root.as_os_str().into(),
                "--output".into(),
                format.into(),
            ]
        };
        run(arguments(root.path()), &mut plain_buffer, ColorMode::Plain);
        run(arguments(root.path()), &mut colored_buffer, colored);
        assert_eq!(plain_buffer, colored_buffer, "{format}");
        assert!(
            !plain_buffer.contains(&0x1b),
            "{format} emitted an ANSI escape"
        );
    }
}

/// An internal error the run survives (here: a subdirectory nobody may
/// list) must read the same across every format: the same kind, the same
/// message, whether or not that format has a dedicated slot for it. JSON
/// carries it as an `internal_errors` entry, SARIF as a
/// `toolExecutionNotification` on the invocation, and GitHub as an
/// `::error::` command with no file property, emitted before the summary.
/// Building this fixture is cheap (the JSON, SARIF and GitHub writer
/// suites already carry it individually, locking a directory to mode
/// `0o000` and restoring the permissions immediately after the run), so
/// this test reuses it to check the three formats agree with each other
/// rather than only with themselves.
#[cfg(unix)]
#[test]
fn every_format_agrees_on_an_internal_error() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = project(&[
        ("src/Kernel.php", "<?php\nclass Kernel {}\n"),
        ("src/locked/Hidden.php", "<?php\nclass Hidden {}\n"),
    ]);
    let locked = root.path().join("src/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let (json_outcome, json) = check_with(root.path(), &["--output", "json"]);
    let (sarif_outcome, sarif) = check_with(root.path(), &["--output", "sarif"]);
    let (github_outcome, github) = check_with(root.path(), &["--output", "github"]);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(json_outcome, Outcome::InternalError, "{json}");
    assert_eq!(sarif_outcome, Outcome::InternalError, "{sarif}");
    assert_eq!(github_outcome, Outcome::InternalError, "{github}");

    let json_value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let json_errors = json_value["internal_errors"].as_array().unwrap();
    assert_eq!(json_errors.len(), 1, "{json}");
    let kind = json_errors[0]["kind"].as_str().unwrap();
    let message = json_errors[0]["message"].as_str().unwrap();
    assert!(!message.is_empty());

    let sarif_value: serde_json::Value = serde_json::from_str(&sarif).unwrap();
    let notifications = sarif_value["runs"][0]["invocations"][0]["toolExecutionNotifications"]
        .as_array()
        .unwrap_or_else(|| panic!("expected toolExecutionNotifications: {sarif}"));
    assert_eq!(notifications.len(), 1, "{sarif}");
    assert_eq!(notifications[0]["descriptor"]["id"].as_str().unwrap(), kind);
    assert_eq!(
        notifications[0]["message"]["text"].as_str().unwrap(),
        message,
        "SARIF's notification text must match JSON's internal_errors message verbatim",
    );

    let expected_github_line = format!("::error::{message}");
    assert!(
        github.lines().any(|line| line == expected_github_line),
        "expected {expected_github_line:?} in: {github}",
    );
}
