//! Warm/cold extended to configuration (closure gate 2): the same
//! `celerrate.toml` gives the warm path byte for byte; a change to its
//! `[rules]` or `[severity]` sections invalidates through the header
//! digest, never through luck.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::Path;

use celerrate_cli::cache::verdict::{VerdictLookup, lookup_verdict};
use celerrate_cli::session::Session;
use celerrate_cli::{ColorMode, run};

const MANIFEST: &str = r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;
/// A file with one stable diagnostic (CEL0018), so verdicts are
/// non-trivial and the report is not empty.
const SOURCE: &str = "<?php\nnamespace App;\n\nnew \\MissingDependency();\n";

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

fn run_check(root: &Path) -> String {
    let mut output = Vec::new();
    let _ = run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.as_os_str().to_owned(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    String::from_utf8(output).unwrap()
}

/// Whether every analyzed file's verdict is served from the packs when
/// a fresh session opens over `root` now.
fn all_verdicts_hit(root: &Path) -> bool {
    let session = Session::start(root);
    let inputs = session.inputs();
    session
        .sources
        .values()
        .all(|&file| matches!(lookup_verdict(&inputs, file), VerdictLookup::Hit { .. }))
}

#[test]
fn the_same_configuration_file_serves_the_warm_path_byte_for_byte() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", SOURCE),
        (
            "celerrate.toml",
            "[rules.null-dereference]\nenabled = true\n",
        ),
    ]);
    let cold = run_check(root.path());
    assert!(all_verdicts_hit(root.path()), "the first run persisted");
    let warm = run_check(root.path());
    assert_eq!(cold, warm, "the warm report is byte-identical");
}

#[test]
fn a_rules_section_change_discards_the_packs() {
    let root = project(&[
        ("composer.json", MANIFEST),
        ("src/Example.php", SOURCE),
        (
            "celerrate.toml",
            "[rules.null-dereference]\nenabled = true\n",
        ),
    ]);
    let _ = run_check(root.path());
    assert!(all_verdicts_hit(root.path()));

    std::fs::write(root.path().join("celerrate.toml"), "").unwrap();
    assert!(
        !all_verdicts_hit(root.path()),
        "a different digest must not serve the old packs",
    );
}

#[test]
fn a_severity_section_change_discards_the_packs() {
    let root = project(&[("composer.json", MANIFEST), ("src/Example.php", SOURCE)]);
    let _ = run_check(root.path());
    assert!(all_verdicts_hit(root.path()));

    std::fs::write(
        root.path().join("celerrate.toml"),
        "[severity]\n\"CEL0018\" = \"warning\"\n",
    )
    .unwrap();
    assert!(!all_verdicts_hit(root.path()));
}

#[test]
fn a_rule_disabled_after_a_warm_run_stops_speaking() {
    // The first run persists verdicts carrying CEL0034; disabling the
    // rule moves the digest, so the second run must not serve them.
    let root = project(&[
        ("composer.json", MANIFEST),
        (
            "src/Consumer.php",
            "<?php\nnamespace App;\n\nclass User { public function save(): void {} }\n\nclass Consumer\n{\n    public function run(?User $maybe): void\n    {\n        $maybe->save();\n    }\n}\n",
        ),
    ]);
    let first = run_check(root.path());
    assert!(first.contains("CEL0034"), "{first}");
    assert!(all_verdicts_hit(root.path()), "the first run persisted");

    std::fs::write(
        root.path().join("celerrate.toml"),
        "[rules.null-dereference]\nenabled = false\n",
    )
    .unwrap();
    let second = run_check(root.path());
    assert!(
        !second.contains("CEL0034"),
        "a stale pack must not resurrect a disabled rule: {second}",
    );
}
