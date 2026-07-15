//! Inline suppressions, end to end: the four written forms extinguish
//! every diagnostic family on their scope, the report and the exit
//! code count the same post-filter set, and nothing leaks across
//! files (design sections 4 and 5).

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
fn a_trailing_ignore_line_extinguishes_the_finding() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
    assert!(!text.contains("CEL0018"), "{text}");
}

#[test]
fn a_hash_comment_carries_the_directive_too() {
    let root = project(&[("a.php", "<?php\nnew MissingOne(); # @phpstan-ignore-line\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn ignore_next_line_targets_the_line_below_and_only_it() {
    let root = project(&[(
        "a.php",
        "<?php\n// @phpstan-ignore-next-line\nnew MissingOne();\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn the_bare_identifier_form_covers_both_of_its_placements() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore class.notFound\n// @phpstan-ignore class.notFound\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn psalm_suppress_on_a_declaration_docblock_covers_its_whole_span() {
    let root = project(&[(
        "a.php",
        "<?php\n/** @psalm-suppress UndefinedClass */\nclass Service\n{\n    public function boot(): void\n    {\n        new MissingOne();\n    }\n}\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn suppression_extinguishes_the_syntax_family_too() {
    // Design section 5: suppression is family-agnostic — exempting the
    // existing families would re-report exactly what it forbids.
    let root = project(&[("a.php", "<?php\n$x = ; // @phpstan-ignore-line\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn a_directive_never_leaks_into_another_file() {
    let root = project(&[
        ("a.php", "<?php\n// @phpstan-ignore-next-line\n"),
        ("b.php", "<?php\nnew MissingOne();\n"),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("MissingOne"), "{text}");
}

#[test]
fn an_unrelated_line_still_reports_beside_a_suppressed_one() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line\nnew MissingTwo();\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}
