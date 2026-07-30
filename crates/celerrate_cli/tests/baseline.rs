//! Baseline integration tests: flags, recording, applying, and its
//! three invariants.

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

fn check(root: &Path) -> (Outcome, String) {
    check_with(root, &[])
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
    let (outcome, text) = check_with(root.path(), &["--baseline"]);
    assert_eq!(outcome, Outcome::Clean);
    assert!(!root.path().join("celerrate-baseline.toml").exists());
    // No write happened, so the report must not claim one did: naming a
    // file that was never created would be a lie the file system disproves
    // on the very next line.
    assert!(
        !text.contains("recorded"),
        "no file was written, so the report must not claim a recording: {text}"
    );
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
    // A write genuinely happened here (the stale entry was dropped), so
    // unlike the untouched-file case above, the report does announce it,
    // with a count of zero entries: the gate is `recorded.is_some()`, not
    // `recorded > 0`, and this is the case that would catch an inverted
    // gate the other test alone could not.
    assert!(
        text.contains("recorded 0 baseline entries"),
        "report was:\n{text}"
    );
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

#[test]
fn a_present_baseline_hides_its_findings_and_the_exit_code() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(
        text.contains("1 baselined diagnostic hidden"),
        "report was:\n{text}"
    );
    assert!(!text.contains("CEL0018"), "report was:\n{text}");
}

#[test]
fn ignore_baseline_runs_strict() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let (outcome, text) = check_with(root.path(), &["--ignore-baseline"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(!text.contains("hidden"), "report was:\n{text}");
}

#[test]
fn an_unreadable_baseline_reports_cel0051_and_runs_strict() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
        ("celerrate-baseline.toml", "version = 1\n[[entry]\n"),
    ]);
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("notice CEL0051"), "report was:\n{text}");
}

#[test]
fn configuration_diagnostics_are_never_hidden_by_a_baseline() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    std::fs::write(
        root.path().join("celerrate.toml"),
        "[rules.no-such-rule]\nenabled = true\n",
    )
    .unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("CEL0046"), "report was:\n{text}");
}

#[test]
fn the_persisted_cache_stays_pre_baseline() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    // Cold run with the baseline applied persists packs; the warm strict
    // run must still report the finding from the cache.
    check(root.path());
    let (outcome, text) = check_with(root.path(), &["--ignore-baseline"]);
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("CEL0018"), "report was:\n{text}");
}

#[test]
fn a_baselined_diagnostic_is_not_fixed_by_fix() {
    // --fix with an applied baseline is legal (only recording conflicts).
    // A hidden finding must not be mutated.
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let before = std::fs::read_to_string(root.path().join("src/Kernel.php")).unwrap();
    let (outcome, _) = check_with(root.path(), &["--fix"]);
    assert_eq!(outcome, Outcome::Clean);
    let after = std::fs::read_to_string(root.path().join("src/Kernel.php")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn an_entry_survives_line_movement() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let moved = "<?php\n\nnamespace App;\n\n// pushed down\n// by two comment lines\nclass Kernel extends Missing\n{\n}\n";
    std::fs::write(root.path().join("src/Kernel.php"), moved).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(
        text.contains("1 baselined diagnostic hidden"),
        "report was:\n{text}"
    );
    assert!(!text.contains("CEL0050"), "report was:\n{text}");
}

#[test]
fn an_entry_dies_with_its_diagnostic_and_is_reported_obsolete() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    std::fs::write(root.path().join("src/Kernel.php"), CLEAN_SOURCE).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("notice CEL0050"), "report was:\n{text}");
    assert!(text.contains("re-record"), "report was:\n{text}");
}

#[test]
fn the_count_never_masks_occurrence_n_plus_one() {
    let two = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        new Missing();\n        new Missing();\n    }\n}\n";
    let three = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        new Missing();\n        new Missing();\n        new Missing();\n    }\n}\n";
    let root = project(&[("composer.json", MANIFEST), ("src/Runner.php", two)]);
    check_with(root.path(), &["--baseline"]);
    std::fs::write(root.path().join("src/Runner.php"), three).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(
        text.contains("2 baselined diagnostics hidden"),
        "report was:\n{text}"
    );
    assert!(text.contains("1 diagnostic"), "report was:\n{text}");
}

#[test]
fn a_renamed_method_resurfaces_its_findings_and_reports_obsolescence() {
    let before = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function run(): void\n    {\n        new Missing();\n    }\n}\n";
    let renamed = "<?php\n\nnamespace App;\n\nclass Runner\n{\n    public function launch(): void\n    {\n        new Missing();\n    }\n}\n";
    let root = project(&[("composer.json", MANIFEST), ("src/Runner.php", before)]);
    check_with(root.path(), &["--baseline"]);
    std::fs::write(root.path().join("src/Runner.php"), renamed).unwrap();
    let (outcome, text) = check(root.path());
    // Noisy but honest, never silent: the finding is back AND the stale
    // entry is announced.
    assert_eq!(outcome, Outcome::DiagnosticsReported, "report was:\n{text}");
    assert!(text.contains("notice CEL0050"), "report was:\n{text}");
}

#[test]
fn a_new_suppression_makes_the_baseline_entry_obsolete() {
    // Filter order: suppression (in-engine), then baseline (CLI). Adding a
    // suppression starves the entry -- the intended behavior.
    //
    // The brief assumed `@celerrate-suppress`; the project's real native
    // directive (see `tests/suppressions.rs`) is `@celerrate-ignore`, and a
    // docblock directive covers the annotated declaration's whole span.
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let suppressed = "<?php\n\nnamespace App;\n\n/** @celerrate-ignore CEL0018 */\nclass Kernel extends Missing\n{\n}\n";
    std::fs::write(root.path().join("src/Kernel.php"), suppressed).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("notice CEL0050"), "report was:\n{text}");
}

#[test]
fn a_partially_parsed_baseline_hides_its_valid_entries_and_reports_cel0051() {
    // A baseline file that only half-parses must not treat its
    // unreadable half as lost capacity --
    // the valid entry still hides its finding, and the broken line is
    // still announced, both at once.
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Kernel.php", FAILING_SOURCE),
    ]);
    check_with(root.path(), &["--baseline"]);
    let mut written = baseline_text(root.path());
    written.push_str("\n[[entry]]\npath = \"src/Ghost.php\"\ncount = 0\n");
    std::fs::write(root.path().join("celerrate-baseline.toml"), written).unwrap();
    let (outcome, text) = check(root.path());
    assert_eq!(outcome, Outcome::Clean, "report was:\n{text}");
    assert!(text.contains("notice CEL0051"), "report was:\n{text}");
    assert!(
        text.contains("1 baselined diagnostic hidden"),
        "report was:\n{text}"
    );
    assert!(!text.contains("CEL0050"), "report was:\n{text}");
}
