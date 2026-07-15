//! Suppression under the persistent cache: the pack stores the
//! post-filter verdict, a warm run serves it parse-free and equal to
//! recomputation, and a directive edit is a plain content-hash miss —
//! stale suppression is structurally impossible (decision 6 of plan
//! 4c: directives are strictly file-local).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::Path;

use celerrate_cli::analysis::composed_diagnostics;
use celerrate_cli::cache::verdict::{VerdictLookup, lookup_verdict};
use celerrate_cli::session::Session;
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

const SUPPRESSED_AND_NOT: &str =
    "<?php\nnew MissingOne(); // @phpstan-ignore-line\nnew MissingTwo();\n";

#[test]
fn the_pack_stores_the_post_filter_verdict_and_serves_it_equal() {
    let root = project(&[("a.php", SUPPRESSED_AND_NOT)]);
    check(root.path());

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let database = &inputs.database;
    let &file = session.sources.values().next().unwrap();

    let VerdictLookup::Hit(stored) = lookup_verdict(&inputs, file) else {
        panic!("the persisted verdict must revalidate on an unchanged project");
    };
    assert_eq!(
        stored.diagnostics.len(),
        1,
        "the suppressed finding never entered the pack",
    );
    assert!(
        stored.diagnostics[0].message.contains("MissingTwo"),
        "{}",
        stored.diagnostics[0].message,
    );

    let file_id = file.file_id(database);
    let content_length = u32::try_from(file.bytes(database).len()).unwrap_or(0);
    let served: Vec<_> = stored
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.to_diagnostic(file_id, content_length).unwrap())
        .collect();
    assert_eq!(
        served,
        composed_diagnostics(&inputs, file),
        "a served verdict must equal recomputation through the shared point",
    );
}

#[test]
fn removing_the_directive_restores_the_finding_on_a_warm_run() {
    let root = project(&[("a.php", SUPPRESSED_AND_NOT)]);
    let (first, _) = check(root.path());
    assert_eq!(first, Outcome::DiagnosticsReported);

    std::fs::write(
        root.path().join("a.php"),
        "<?php\nnew MissingOne();\nnew MissingTwo();\n",
    )
    .unwrap();
    let (second, text) = check(root.path());
    assert_eq!(second, Outcome::DiagnosticsReported);
    assert!(text.contains("MissingOne"), "{text}");
    assert!(text.contains("MissingTwo"), "{text}");
}

#[test]
fn a_warm_run_over_an_unchanged_project_stays_suppressed() {
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @phpstan-ignore-line\n",
    )]);
    let (cold, _) = check(root.path());
    assert_eq!(cold, Outcome::Clean);
    let (warm, text) = check(root.path());
    assert_eq!(warm, Outcome::Clean, "{text}");
    assert!(!text.contains("MissingOne"), "{text}");
}
