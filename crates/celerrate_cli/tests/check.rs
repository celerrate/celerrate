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

/// A typo'd path that exits 0 is the one thing a CI-facing checker must
/// never do: the build goes green over a project nothing ever looked at.
/// It used to fall through to zero-configuration discovery, announce that
/// it was analyzing a directory it had never been handed, analyze nothing,
/// and succeed.
#[test]
fn a_root_that_does_not_exist_is_a_usage_error_that_names_it() {
    let mut output = Vec::new();
    let outcome = run(
        vec![
            "celerrate".into(),
            "check".into(),
            "/nonexistent/path/xyz".into(),
        ],
        &mut output,
    );
    let text = String::from_utf8(output).unwrap();

    assert_eq!(outcome, Outcome::UsageError, "{text}");
    assert!(
        text.contains("/nonexistent/path/xyz"),
        "the message names the path the user gave: {text}",
    );
    assert!(
        !text.contains("CEL0025"),
        "and it does not announce a fallback it never took: {text}",
    );
}

#[test]
fn a_root_that_is_a_file_rather_than_a_directory_is_a_usage_error() {
    let root = project(&[("a.php", "<?php echo 1;")]);
    let file = root.path().join("a.php");
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), file.as_os_str().into()],
        &mut output,
    );
    let text = String::from_utf8(output).unwrap();

    assert_eq!(outcome, Outcome::UsageError, "{text}");
    assert!(text.contains("a.php"), "{text}");
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

/// A real Composer project has thousands of third-party files, and a
/// report dominated by their findings is no report at all: they are not
/// the user's code, not the user's to fix, and failing the build on them
/// is failing it on someone else's work.
///
/// They must still be analyzed, though. Their symbols are exactly what
/// makes `use Acme\Thing;` resolve, which is what the `Kernel` here
/// proves: drop the vendor files from the analyzed set and it reports an
/// unknown class instead, so this test cannot pass by silencing vendor
/// the lazy way.
///
/// And the count that reaches the exit code must be the count that was
/// printed. A vendor finding that exits 1 over an empty report is worse
/// than either half of the bug.
#[test]
fn a_finding_in_vendor_is_analyzed_but_never_reported() {
    let root = project(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "vendor/composer/installed.json",
            r#"{"packages": [{"name": "acme/lib", "install-path": "../acme/lib",
               "autoload": {"psr-4": {"Acme\\": "src/"}}}]}"#,
        ),
        (
            "vendor/acme/lib/src/Broken.php",
            "<?php\nnamespace Acme;\nclass Broken extends TotallyMissing {}\n",
        ),
        (
            "vendor/acme/lib/src/Thing.php",
            "<?php\nnamespace Acme;\nclass Thing {}\n",
        ),
        (
            "src/Kernel.php",
            "<?php\nnamespace App;\nuse Acme\\Thing;\nclass Kernel extends Thing {}\n",
        ),
    ]);
    let (outcome, text) = check(root.path());

    assert!(
        !text.contains("TotallyMissing"),
        "a third-party finding is not the user's to fix: {text}",
    );
    assert!(
        !text.contains("vendor"),
        "nothing from vendor reaches the report at all: {text}",
    );
    assert!(text.contains("0 diagnostics"), "{text}");
    assert_eq!(
        outcome,
        Outcome::Clean,
        "the count the exit code is derived from is the count that was printed: {text}",
    );
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
