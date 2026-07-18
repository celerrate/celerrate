//! The revalidation-sufficiency net (audit finding I2): for every file
//! whose records all revalidate, the diagnostics the pack serves must
//! equal, value for value, what a full recomputation produces. Sound
//! today because every reference check is a pure function of the
//! recorded answers plus the header-pinned range; the first future
//! check that reads more than the answer captures — a declaration's
//! kind, its defining file, index-global state — fails here, on a warm
//! run, instead of silently serving wrong diagnostics.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::path::Path;

use celerrate_cli::analysis::{composed_diagnostics, served_typed_diagnostics};
use celerrate_cli::cache::verdict::{TypedOutcome, VerdictLookup, lookup_verdict};
use celerrate_cli::run;
use celerrate_cli::session::Session;

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

fn run_check(root: &Path) {
    let mut output = Vec::new();
    let _ = run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.as_os_str().to_owned(),
        ],
        &mut output,
    );
}

/// Analyzes and persists in one process, restarts a session over the
/// packs, and for every file whose verdict revalidates asserts the
/// served diagnostics equal a recomputation through the shared
/// composition point. Answers the set of diagnostic identifiers the
/// served verdicts carried, so callers can assert the fixture really
/// exercised the intended answer shapes rather than validating nothing.
fn served_equals_recomputed(files: &[(&str, &str)]) -> BTreeSet<String> {
    let root = project(files);
    run_check(root.path());

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let database = &inputs.database;
    let mut served_identifiers = BTreeSet::new();
    let mut validated = 0;
    for &file in session.sources.values() {
        let VerdictLookup::Hit {
            verdict: stored,
            typed,
        } = lookup_verdict(&inputs, file)
        else {
            continue;
        };
        validated += 1;
        let file_id = file.file_id(database);
        let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
        let served: Option<Vec<_>> = stored
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length))
            .collect();
        let mut served = served.expect("a revalidated verdict's diagnostics all convert");
        // The typed half is layered independently (plan 9a, task 9): a
        // served typed outcome speaks from `stored.typed`, a recomputed
        // one falls through to a fresh `typed_portion` — through
        // `served_typed_diagnostics`, the exact function `analyze_one`
        // itself calls on a hit, so the two compositions cannot
        // independently drift.
        let typed_source = match typed {
            TypedOutcome::Served => stored.typed.as_ref(),
            TypedOutcome::Recompute => None,
        };
        served.extend(served_typed_diagnostics(&inputs, file, typed_source));
        served.sort();
        let recomputed = composed_diagnostics(&inputs, file);
        assert_eq!(
            served, recomputed,
            "a validated verdict must equal what recomputation produces",
        );
        for diagnostic in &served {
            served_identifiers.insert(diagnostic.id.as_str().to_owned());
        }
    }
    assert!(
        validated > 0,
        "the fixture produced no validated verdict: the net caught nothing",
    );
    served_identifiers
}

/// Source resolution (no diagnostic), unknown class (CEL0018), unknown
/// function (CEL0019), unknown constant (CEL0020).
#[test]
fn source_and_unknown_answers_replay_equal() {
    let identifiers = served_equals_recomputed(&[(
        "a.php",
        "<?php class Known {} new Known(); new Missing(); absent_function(); echo ABSENT_CONSTANT;",
    )]);
    for expected in ["CEL0018", "CEL0019", "CEL0020"] {
        assert!(
            identifiers.contains(expected),
            "the fixture must exercise {expected}: {identifiers:?}",
        );
    }
}

/// Stub answers with an availability window: a symbol introduced after
/// the project's minimum (CEL0021) and a symbol deprecated within the
/// range (CEL0023), beside an always-available stub answer (`strlen`,
/// no diagnostic). If either identifier is missing, the chosen stub
/// symbol's metadata differs from the embedded snapshot's — pick
/// another symbol carrying the same window shape rather than weakening
/// the assertion.
#[test]
fn stub_window_answers_replay_equal() {
    let identifiers = served_equals_recomputed(&[
        ("composer.json", r#"{"require": {"php": ">=8.1"}}"#),
        (
            "a.php",
            "<?php strlen('x'); json_validate('{}'); utf8_encode('x');",
        ),
    ]);
    for expected in ["CEL0021", "CEL0023"] {
        assert!(
            identifiers.contains(expected),
            "the fixture must exercise {expected}: {identifiers:?}",
        );
    }
}

/// A multi-file project where answers cross files: the consumer's
/// verdict records a `Source` answer for a class another file declares.
#[test]
fn cross_file_source_answers_replay_equal() {
    served_equals_recomputed(&[
        ("src/Consumer.php", "<?php new Widget(); new Gone();"),
        ("src/Widget.php", "<?php class Widget {}"),
    ]);
}

/// A typed finding (CEL0034, a possibly-null dereference): the pack now
/// persists it too (plan 9a, task 9), so a warm second pass over an
/// unchanged project must serve it back rather than recompute — the net
/// still covers the full union (untyped plus typed), the same way
/// `analyze_one` does on a warm hit, through `served_typed_diagnostics`.
#[test]
fn typed_answers_replay_equal() {
    let identifiers = served_equals_recomputed(&[
        (
            "composer.json",
            r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        ),
        (
            "src/Service.php",
            "<?php\ndeclare(strict_types=1);\nnamespace App;\n\nclass User { public function save(): void {} }\n\nclass Service\n{\n    public function run(?User $user): void\n    {\n        $user->save();\n    }\n}\n",
        ),
    ]);
    assert!(
        identifiers.contains("CEL0034"),
        "the fixture must exercise a typed finding: {identifiers:?}",
    );
}
