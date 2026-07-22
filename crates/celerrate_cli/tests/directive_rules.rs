//! The CEL0041/CEL0042 product matrix: the directive rules through
//! the full pipeline, native directives only, one-pass suppression
//! discipline (design sections 8 and 11).

#![allow(clippy::unwrap_used)]

use std::path::Path;

use celerrate_cli::cache::snapshot::DIAGNOSTICS_PACK;
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
fn a_typo_in_a_native_directive_reports_cel0041_and_the_finding_survives() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0019, CEL9999\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0041"), "{text}");
    assert!(text.contains("CEL9999"), "{text}");
    assert!(text.contains("CEL0018"), "{text}");
}

#[test]
fn an_unused_native_directive_reports_cel0042() {
    let root = project(&[("a.php", "<?php\n$x = 1; // @celerrate-ignore CEL0018\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0042"), "{text}");
}

#[test]
fn a_warm_second_check_reports_cel0042_identically_to_the_cold_first() {
    // The product matrix above drives every fixture through `check`
    // exactly once, so only the cold, recompute path is pinned through
    // the real `run` entry point. Checking the same, unmodified root
    // twice exercises the warm path too: the first call persists a
    // verdict pack, and the second loads a session over that same root,
    // which serves from it rather than recomputing (mirrored by
    // `cache_seeding.rs`'s `a_second_run_leaves_equivalent_packs_behind`,
    // the established way this codebase pins a warm run).
    let root = project(&[("a.php", "<?php\n$x = 1; // @celerrate-ignore CEL0018\n")]);

    let (first_outcome, first_text) = check(root.path());
    assert_eq!(first_outcome, Outcome::DiagnosticsReported, "{first_text}");
    assert!(first_text.contains("CEL0042"), "{first_text}");

    // The first check must have persisted a diagnostics pack, or the
    // second check below would have nothing to warm from and would
    // silently recompute instead.
    assert!(
        root.path()
            .join(".celerrate/cache")
            .join(DIAGNOSTICS_PACK)
            .is_file(),
        "the first check must persist a diagnostics pack for the second \
         check to serve the directive diagnostics warm",
    );

    let (second_outcome, second_text) = check(root.path());
    assert_eq!(
        second_outcome,
        Outcome::DiagnosticsReported,
        "{second_text}"
    );
    assert!(second_text.contains("CEL0042"), "{second_text}");
}

#[test]
fn a_used_native_directive_is_clean() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0018\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn a_bare_native_directive_reports_cel0042() {
    let root = project(&[("a.php", "<?php\n$x = 1; // @celerrate-ignore\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0042"), "{text}");
}

#[test]
fn a_native_directive_alone_on_the_last_line_reports_cel0042() {
    // No next line exists: the scope degenerates to the empty
    // end-of-file range (decision 6), nothing is suppressed, and the
    // directive is still visible to the reporting rules.
    let root = project(&[("a.php", "<?php\n$x = 1;\n// @celerrate-ignore CEL0018")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0042"), "{text}");
}

#[test]
fn an_unused_foreign_directive_is_never_reported() {
    let root = project(&[("a.php", "<?php\n$x = 1; // @phpstan-ignore-line\n")]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn suppressing_a_directive_diagnostic_counts_as_use_of_the_suppressor() {
    // Line 3's directive suppressed nothing on its own scope; its
    // CEL0042 is suppressed by line 2's directive targeting the next
    // line with CEL0042 - which thereby counts as used and reports
    // nothing itself. One pass, clean run.
    let root = project(&[(
        "a.php",
        "<?php\n// @celerrate-ignore CEL0042\n$x = 1; // @celerrate-ignore CEL0018\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn cel0041_is_itself_suppressible() {
    // The foreign tag comes first (the native identifier list runs to
    // the end of the line). The foreign blanket admits the CEL0041
    // aimed at the typo, so the run is clean.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line @celerrate-ignore CEL0018, CEL9999\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}

#[test]
fn any_match_attribution_marks_every_admitting_directive_used() {
    // Two co-located directives (separate comments) both admit the one
    // CEL0018: both are used, neither reports CEL0042.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); /* @celerrate-ignore CEL0018 */ // @phpstan-ignore class.notFound\n",
    )]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "{text}");
}
