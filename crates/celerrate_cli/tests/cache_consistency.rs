//! The cross-process extension of the incremental correctness harness:
//! edit sequences replayed over a project on disk, with every
//! cache-seeded run asserted byte-for-byte identical to a from-scratch
//! run over the same state. Nothing survives between runs except
//! `.celerrate/cache/`, which is exactly the boundary under test.

#![allow(clippy::unwrap_used)]
#![allow(clippy::indexing_slicing)]

use std::path::Path;

use celerrate_cli::run;

fn run_check(root: &Path) -> String {
    let mut output = Vec::new();
    let _ = run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.as_os_str().to_owned(),
        ],
        &mut output,
    );
    String::from_utf8(output).unwrap()
}

/// The rendering is root-relative, but notices and internal errors may
/// name absolute paths: normalize both roots to one marker before
/// comparing.
fn normalized(output: &str, root: &Path) -> String {
    output.replace(&root.display().to_string(), "<root>")
}

/// Copies the project, excluding the cache: the from-scratch twin.
fn copy_without_cache(source: &Path, destination: &Path) {
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == ".celerrate" {
            continue;
        }
        let target = destination.join(&name);
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_without_cache(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// One step of an edit sequence.
enum Step {
    Write(&'static str, &'static str),
    Delete(&'static str),
}

/// Replays the steps over one cached project directory; after the
/// initial state and after every step, the cached run must render what
/// a from-scratch run over a cache-free copy renders.
fn assert_cached_matches_fresh(initial: &[(&str, &str)], steps: &[Step]) {
    let cached = tempfile::tempdir().unwrap();
    for (path, contents) in initial {
        let path = cached.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    let assert_state_matches = |label: &str| {
        let cached_output = run_check(cached.path());
        let fresh = tempfile::tempdir().unwrap();
        copy_without_cache(cached.path(), fresh.path());
        let fresh_output = run_check(fresh.path());
        assert_eq!(
            normalized(&cached_output, cached.path()),
            normalized(&fresh_output, fresh.path()),
            "cached and from-scratch renderings diverged {label}",
        );
    };

    // The first run both checks the cold state and writes the cache;
    // the second checks the warm no-change state.
    assert_state_matches("on the cold state");
    assert_state_matches("on the warm unchanged state");

    for (index, step) in steps.iter().enumerate() {
        match step {
            Step::Write(path, contents) => {
                let path = cached.path().join(path);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, contents).unwrap();
            }
            Step::Delete(path) => {
                std::fs::remove_file(cached.path().join(path)).unwrap();
            }
        }
        assert_state_matches(&format!("after step {index}"));
    }
}

#[test]
fn body_and_comment_edits_replay_consistently() {
    assert_cached_matches_fresh(
        &[
            (
                "src/Service.php",
                "<?php class Service { public function run() { return 1; } }",
            ),
            ("src/User.php", "<?php class User {}"),
        ],
        &[
            Step::Write(
                "src/Service.php",
                "<?php class Service { public function run() { return 2; } }",
            ),
            Step::Write(
                "src/Service.php",
                "<?php /* documented */ class Service { public function run() { return 2; } }",
            ),
        ],
    );
}

/// The stale-verdict trap in both directions: a cached unknown-symbol
/// diagnostic must die when a defining file appears, and come back
/// when it goes.
#[test]
fn a_definition_appearing_and_vanishing_replays_consistently() {
    assert_cached_matches_fresh(
        &[("src/Consumer.php", "<?php new Missing();")],
        &[
            Step::Write("src/Definer.php", "<?php class Missing {}"),
            Step::Delete("src/Definer.php"),
        ],
    );
}

/// A signature-level edit in one file must be seen by the cached
/// verdicts of another: renaming the declared class flips its
/// consumers' resolution.
#[test]
fn a_rename_in_another_file_replays_consistently() {
    assert_cached_matches_fresh(
        &[
            ("src/Consumer.php", "<?php new Widget();"),
            ("src/Widget.php", "<?php class Widget {}"),
        ],
        &[
            Step::Write("src/Widget.php", "<?php class Renamed {}"),
            Step::Write("src/Widget.php", "<?php class Widget {}"),
        ],
    );
}

/// Composer projects: a vendor file's symbols resolve from the cache
/// like from source, and vendor diagnostics stay unreported.
///
/// `ProjectDiscovery` only walks a vendor package that `installed.json`
/// actually declares (name, `install-path`, autoload): an empty package
/// list, as the task brief's fixture had it, walks nothing, so the
/// vendor file would neither define `Helper` nor be parsed at all,
/// defeating the fixture's intent. This mirrors the package shape
/// `crates/celerrate_project/tests/discovery_end_to_end.rs` and
/// `crates/celerrate_cli/tests/check.rs` use for an installed
/// dependency, with `install-path` chosen so the walk root lands on
/// exactly `vendor/lib/src`, keeping the brief's file path unchanged.
#[test]
fn a_composer_project_replays_consistently() {
    assert_cached_matches_fresh(
        &[
            (
                "composer.json",
                r#"{"require": {"php": "^8.2"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            (
                "vendor/lib/src/Helper.php",
                "<?php namespace Lib; class Helper { public function broken( }",
            ),
            (
                "vendor/composer/installed.json",
                r#"{"packages": [{"name": "acme/lib", "install-path": "../lib",
                   "autoload": {"psr-4": {"Lib\\": "src/"}}}]}"#,
            ),
            (
                "src/App.php",
                "<?php namespace App; use Lib\\Helper; new Helper();",
            ),
        ],
        &[Step::Write(
            "src/App.php",
            "<?php namespace App; use Lib\\Helper; new Helper(); new Gone();",
        )],
    );
}

/// Every corruption mode of a pack on disk regenerates silently: the
/// run's rendering never changes.
#[test]
fn corrupted_packs_never_change_the_rendering() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.php"), "<?php new Missing();").unwrap();
    let baseline = normalized(&run_check(root.path()), root.path());

    let cache = root.path().join(".celerrate/cache");
    for pack in ["item_trees.bin", "diagnostics.bin"] {
        let path = cache.join(pack);
        let original = std::fs::read(&path).unwrap();

        // Truncated.
        std::fs::write(&path, &original[..original.len() / 2]).unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);

        // Garbage.
        std::fs::write(&path, b"not a pack at all").unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);

        // A flipped byte deep in the payload.
        let mut flipped = std::fs::read(&path).unwrap();
        if let Some(last) = flipped.last_mut() {
            *last ^= 0xFF;
        }
        std::fs::write(&path, &flipped).unwrap();
        assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);
    }

    // After all that abuse the packs are healthy again: one more
    // clean pair of runs.
    assert_eq!(normalized(&run_check(root.path()), root.path()), baseline);
}
