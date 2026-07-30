//! The autofix engine, end to end: flags, application, the trailer,
//! and its own honesty pins.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{ColorMode, Outcome, run};

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
    let mut arguments: Vec<std::ffi::OsString> =
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()];
    arguments.extend(extra.iter().map(Into::into));
    let mut output = Vec::new();
    let outcome = run(arguments, &mut output, ColorMode::Plain);
    (outcome, String::from_utf8(output).unwrap())
}

fn typo_project() -> tempfile::TempDir {
    project(&[
        ("composer.json", MANIFEST),
        (
            "src/User.php",
            "<?php\nnamespace App;\nclass User { public function save(): void {} }\n",
        ),
        (
            "src/Caller.php",
            "<?php\nnamespace App;\nfunction persist(User $user): void { $user->svae(); }\n",
        ),
    ])
}

/// The central promise: `--fix-suggestions` patches the file,
/// and re-checking the patched project no longer reports the fixed
/// diagnostic (the fix-closes-the-diagnostic property).
#[test]
fn fix_suggestions_patches_the_typo_and_the_recheck_is_clean() {
    let root = typo_project();
    let (outcome, text) = check_with(root.path(), &["--fix-suggestions"]);
    // The run still reports what it found: no fixpoint, exit 1.
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    assert!(text.contains("applied 1 fix to 1 file"), "{text}");
    let patched = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    insta::assert_snapshot!("patched_caller", patched);
    assert!(patched.contains("$user->save()"), "{patched}");
    let (recheck, _) = check_with(root.path(), &[]);
    assert_eq!(recheck, Outcome::Clean, "the fix closes the diagnostic");
}

/// The owned consequence, pinned honestly: every natural fix is
/// `NeedsReview`, so `--fix` alone applies nothing at closure.
#[test]
fn fix_alone_applies_nothing_at_closure() {
    let root = typo_project();
    let before = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    let (outcome, text) = check_with(root.path(), &["--fix"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    assert!(text.contains("applied 0 fixes to 0 files"), "{text}");
    let after = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    assert_eq!(before, after, "the file is untouched");
}

/// The ambiguity discipline: a tie produces a note, never an edit, and
/// bulk application leaves the file alone.
#[test]
fn an_ambiguous_candidate_is_listed_and_never_applied() {
    let root = project(&[
        ("composer.json", MANIFEST),
        (
            "src/User.php",
            "<?php\nnamespace App;\nclass User { public function save(): void {} public function sove(): void {} }\n",
        ),
        (
            "src/Caller.php",
            "<?php\nnamespace App;\nfunction f(User $user): void { $user->sive(); }\n",
        ),
    ]);
    let before = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    let (outcome, text) = check_with(root.path(), &["--fix-suggestions"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported);
    assert!(
        text.contains("note: did you mean one of `save`, `sove`?"),
        "{text}",
    );
    assert!(text.contains("applied 0 fixes to 0 files"), "{text}");
    let after = std::fs::read_to_string(root.path().join("src/Caller.php")).unwrap();
    assert_eq!(before, after);
}

/// An unknown symbol rides the same pass: the class typo is patched
/// under `--fix-suggestions`.
#[test]
fn an_unknown_class_typo_is_patched_too() {
    let root = project(&[
        ("composer.json", MANIFEST),
        (
            "src/Gateway.php",
            "<?php\nnamespace App;\nclass PaymentGateway {}\n",
        ),
        (
            "src/Consumer.php",
            "<?php\nnamespace App;\nnew PaymentGatewya();\n",
        ),
    ]);
    let (_, text) = check_with(root.path(), &["--fix-suggestions"]);
    assert!(text.contains("applied 1 fix to 1 file"), "{text}");
    let patched = std::fs::read_to_string(root.path().join("src/Consumer.php")).unwrap();
    assert!(patched.contains("new PaymentGateway()"), "{patched}");
    let (recheck, _) = check_with(root.path(), &[]);
    assert_eq!(recheck, Outcome::Clean);
}

/// clap enforces the conflict; the product exits 2 with clap's own
/// message, like every other usage error.
#[test]
fn a_fix_flag_with_watch_exits_two() {
    let root = typo_project();
    let (outcome, text) = check_with(root.path(), &["--fix", "--watch"]);
    assert_eq!(outcome, Outcome::UsageError);
    assert!(text.contains("--watch"), "{text}");
}
