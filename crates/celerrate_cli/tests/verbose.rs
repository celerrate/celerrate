//! The verbose channel end to end: stdout is byte-identical with and
//! without `--verbose` in every output format — the flag speaks only
//! on stderr, so the machine formats cannot move (the spec's
//! transverse decision, pinned here).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::{ColorMode, Outcome, run};

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

fn check(root: &Path, extra: &[&str]) -> (Outcome, Vec<u8>) {
    let mut output = Vec::new();
    let mut arguments: Vec<std::ffi::OsString> =
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()];
    arguments.extend(extra.iter().map(|argument| argument.into()));
    let outcome = run(arguments, &mut output, ColorMode::Plain);
    (outcome, output)
}

#[test]
fn stdout_is_byte_identical_with_and_without_verbose_in_every_format() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore some.unknownIdentifier\nnew MissingTwo();\n",
    )]);
    // One throwaway run so both compared runs are equally warm: the
    // report is warm/cold byte-identical by contract, but this test
    // must not depend on that contract to isolate its own claim.
    let _ = check(root.path(), &[]);
    for format in ["human", "json", "sarif", "github"] {
        let (outcome_without, without) = check(root.path(), &["--output", format]);
        let (outcome_with, with) = check(root.path(), &["--output", format, "--verbose"]);
        assert_eq!(outcome_without, outcome_with, "{format}");
        assert_eq!(without, with, "{format}: stdout must not move");
    }
}

#[test]
fn verbose_with_a_machine_format_is_not_a_usage_error() {
    let root = project(&[("a.php", "<?php\n$clean = 1;\n")]);
    let (outcome, _) = check(root.path(), &["--output", "json", "--verbose"]);
    assert_eq!(outcome, Outcome::Clean);
}
