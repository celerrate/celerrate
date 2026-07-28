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

const FAILING_SOURCE: &str = "<?php\n\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n";

fn baseline_text(root: &Path) -> String {
    std::fs::read_to_string(root.join("celerrate-baseline.toml")).unwrap()
}

#[test]
fn recording_writes_a_sorted_versioned_file_and_exits_clean() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    let (outcome, text) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(
        text.contains("recorded 1 baseline entry"),
        "report was:\n{text}"
    );
    let written = baseline_text(root.path());
    assert!(written.contains("version = 1"), "file was:\n{written}");
    assert!(
        written.contains("path = \"src/Kernel.php\""),
        "file was:\n{written}"
    );
    // `toml_edit` prefers a literal (single-quoted) string over escaping
    // the backslash in a basic string, so `App\Kernel` serializes as
    // `'App\Kernel'`, not `"App\\Kernel"` as a naive reading of the TOML
    // spec's escaping rules would suggest.
    assert!(
        written.contains("symbol = 'App\\Kernel'"),
        "file was:\n{written}"
    );
    assert!(written.contains("count = 1"), "file was:\n{written}");
}

#[test]
fn recording_twice_is_byte_identical() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let first = baseline_text(root.path());
    check_with(root.path(), &["--baseline"]);
    assert_eq!(first, baseline_text(root.path()));
}

#[test]
fn recording_a_clean_project_writes_no_file() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
    ]);
    let (outcome, _) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean);
    assert!(!root.path().join("celerrate-baseline.toml").exists());
}

#[test]
fn recording_a_now_clean_project_rewrites_the_existing_file_header_only() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", CLEAN_SOURCE),
        (
            "celerrate-baseline.toml",
            "version = 1\n\n[[entry]]\npath = \"src/Old.php\"\nidentifier = \"CEL0018\"\nsymbol = \"(top level)\"\nmessage = \"m\"\ncount = 1\n",
        ),
    ]);
    let (outcome, text) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    let written = baseline_text(root.path());
    assert!(written.contains("version = 1"));
    assert!(!written.contains("src/Old.php"), "file was:\n{written}");
}

#[test]
fn duplicate_occurrences_aggregate_into_one_entry_with_a_count() {
    let source = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        new Missing();\n        new Missing();\n    }\n}\n";
    let root = project(&[("composer.json", MANIFEST), ("src/Runner.php", source)]);
    let (outcome, _) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean);
    let written = baseline_text(root.path());
    assert!(written.contains("count = 2"), "file was:\n{written}");
    assert_eq!(
        written.matches("[[entry]]").count(),
        1,
        "file was:\n{written}"
    );
}

#[test]
fn configuration_diagnostics_are_never_recorded_and_still_fail_the_run() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
        ("celerrate.toml", "[rules.no-such-rule]\nenabled = true\n"),
    ]);
    let (outcome, text) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("CEL0046"), "report was:\n{text}");
    let written = baseline_text(root.path());
    assert!(!written.contains("CEL0046"), "file was:\n{written}");
}
