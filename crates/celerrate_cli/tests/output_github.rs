//! `--output=github`: GitHub Actions workflow commands over the same
//! final stream as the human renderer, the JSON writer and the SARIF
//! writer.
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
// Declares the PHP version so the "no PHP version configured" project
// notice never fires: a project this clean must produce no annotation at
// all, only the summary.
const MANIFEST_WITH_PHP_VERSION: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
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
fn findings_become_error_annotations_with_positions() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "github"]);
    let annotations: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("::error") || line.starts_with("::warning"))
        .collect();
    assert!(!annotations.is_empty());
    for annotation in &annotations {
        assert!(annotation.contains("file=src/Example.php"), "{annotation}");
        assert!(annotation.contains("line="), "{annotation}");
        assert!(annotation.contains("col="), "{annotation}");
        assert!(annotation.contains("::CEL"), "{annotation}");
    }
}

#[test]
fn notices_become_notice_commands() {
    let root = project(&[("src/Example.php", FAILING_EXAMPLE)]);
    let (_, text) = check_with(root.path(), &["--output", "github"]);
    assert!(text.lines().any(|line| line.starts_with("::notice::CEL")));
}

#[test]
fn the_summary_closes_the_output() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "github"]);
    let last = text.lines().last().unwrap();
    assert!(last.contains("diagnostic"), "{last}");
    assert!(!last.starts_with("::"), "{last}");
}

#[test]
fn github_findings_snapshot() {
    let root = findings_project();
    let (_, text) = check_with(root.path(), &["--output", "github"]);
    insta::assert_snapshot!("github_findings", text);
}

#[test]
fn a_clean_project_reports_only_the_summary() {
    let root = project(&[
        ("composer.json", MANIFEST_WITH_PHP_VERSION),
        ("src/Clean.php", CLEAN_EXAMPLE),
    ]);
    let (outcome, text) = check_with(root.path(), &["--output", "github"]);
    assert_eq!(outcome, Outcome::Clean, "{text}");
    assert!(!text.lines().any(|line| line.starts_with("::")), "{text}");
    let last = text.lines().last().unwrap();
    assert!(last.contains("0 diagnostic"), "{last}");
}

#[test]
fn machine_output_refuses_the_mutating_and_looping_flags() {
    let root = findings_project();
    for flag in ["--watch", "--fix", "--fix-suggestions", "--baseline"] {
        let (outcome, text) = check_with(root.path(), &["--output", "github", flag]);
        assert_eq!(outcome, Outcome::UsageError, "{flag}");
        assert!(text.contains(flag), "{flag}: {text}");
        assert!(text.contains("--output=github"), "{flag}: {text}");
    }
}

/// A baseline that hides a finding must still be counted: the "N baselined
/// diagnostic(s) hidden" line is the only place that count survives, and
/// nothing exercised it before this test (every hand-built `MachineReport`
/// unit test uses `baselined_hidden: 0`, and no other integration test
/// records a baseline). Run through the real baseline path, not a
/// hand-built report, so the count surviving from `--baseline` recording
/// through to the `github` writer is proven end to end.
#[test]
fn an_applied_baseline_reports_the_hidden_count_after_the_summary() {
    let root = findings_project();
    // Recording under the human (default) output is not the disallowed
    // combination `machine_output_refuses_the_mutating_and_looping_flags`
    // guards: that guard is about recording under a machine format, not
    // about recording at all.
    let (_, _) = check_with(root.path(), &["--baseline"]);
    let (outcome, text) = check_with(root.path(), &["--output", "github"]);
    assert_eq!(outcome, Outcome::Clean, "{text}");

    let lines: Vec<&str> = text.lines().collect();
    let last_index = lines.len() - 1;
    let hidden_line = lines[last_index];
    assert_eq!(
        hidden_line, "1 baselined diagnostic hidden",
        "the hidden count must carry the same wording the human report \
         prints, and must close the output: {text}",
    );
    let summary_index = last_index - 1;
    assert!(
        lines[summary_index].contains("diagnostic") && !lines[summary_index].starts_with("::"),
        "the hidden line must come right after the summary line: {text}",
    );
    assert!(!hidden_line.starts_with("::"), "{hidden_line}");
}

/// An internal error the run survives must appear as an `::error::`
/// workflow command carrying the same message the JSON and SARIF writers
/// carry, placed after the diagnostics and before the summary so the
/// summary still genuinely closes the output.
#[cfg(unix)]
#[test]
fn an_internal_error_becomes_an_error_annotation_before_the_summary() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = project(&[
        ("src/Kernel.php", "<?php\nclass Kernel {}\n"),
        ("src/locked/Hidden.php", "<?php\nclass Hidden {}\n"),
    ]);
    let locked = root.path().join("src/locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let (outcome, text) = check_with(root.path(), &["--output", "github"]);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(outcome, Outcome::InternalError, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    let error_index = lines
        .iter()
        .position(|line| line.starts_with("::error::") && line.contains("could not be read"))
        .unwrap_or_else(|| panic!("expected an internal-error annotation: {text}"));
    assert!(
        !lines[error_index].contains("file="),
        "an internal error carries no file property: {}",
        lines[error_index],
    );
    let last_index = lines.len() - 1;
    assert!(
        error_index < last_index,
        "the internal error must come before the summary: {text}",
    );
    assert!(!lines[last_index].starts_with("::"), "{text}");
    assert!(lines[last_index].contains("diagnostic"), "{text}");
}
