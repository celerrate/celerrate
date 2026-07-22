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

    let VerdictLookup::Hit {
        verdict: stored, ..
    } = lookup_verdict(&inputs, file)
    else {
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
fn the_pack_stores_the_directive_match_records() {
    let root = project(&[("a.php", SUPPRESSED_AND_NOT)]);
    check(root.path());

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let &file = session.sources.values().next().unwrap();
    let VerdictLookup::Hit {
        verdict: stored, ..
    } = lookup_verdict(&inputs, file)
    else {
        panic!("the persisted verdict must revalidate on an unchanged project");
    };
    let content_length = u32::try_from(file.bytes(&inputs.database).len()).unwrap_or(0);
    let records = stored
        .directives_convert(content_length)
        .expect("stored directive records convert");
    assert_eq!(records.len(), 1);
    let (_, matched) = &records[0];
    assert!(*matched, "the ignore-line directive admitted MissingOne");
    assert_eq!(
        records
            .iter()
            .map(|(directive, matched)| (directive.clone(), *matched))
            .collect::<Vec<_>>(),
        celerrate_semantics::suppression_directives(&inputs.database, file)
            .iter()
            .cloned()
            .map(|fresh| (fresh, true))
            .collect::<Vec<_>>(),
        "stored records equal the query plus the match outcome",
    );
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

#[test]
fn a_warm_run_reports_the_same_directive_diagnostics_from_the_records() {
    // Cold: the unused native directive reports CEL0042. Warm: the
    // verdict serves, the reporting phase replays from the stored
    // match records, and the report is byte-identical.
    let root = project(&[("a.php", "<?php\n$x = 1; // @celerrate-ignore CEL0018\n")]);
    let (cold_outcome, cold_text) = check(root.path());
    assert_eq!(cold_outcome, Outcome::DiagnosticsReported, "{cold_text}");
    assert!(cold_text.contains("CEL0042"), "{cold_text}");

    let (warm_outcome, warm_text) = check(root.path());
    assert_eq!(warm_outcome, Outcome::DiagnosticsReported, "{warm_text}");
    assert_eq!(
        cold_text, warm_text,
        "warm and cold reports must be byte-identical"
    );
}

#[test]
fn the_warm_replay_serves_the_verdict_rather_than_recomputing() {
    // The parse-free claim, by elimination: the stored diagnostics
    // never contain CEL0042, yet the warm run reports it - so the
    // reporting phase ran from the stored match records, not from a
    // persisted diagnostic.
    let root = project(&[("a.php", "<?php\n$x = 1; // @celerrate-ignore CEL0018\n")]);
    check(root.path());

    let session = Session::start(root.path());
    let inputs = session.inputs();
    let &file = session.sources.values().next().unwrap();
    let VerdictLookup::Hit {
        verdict: stored, ..
    } = lookup_verdict(&inputs, file)
    else {
        panic!("the persisted verdict must revalidate on an unchanged project");
    };
    assert!(
        stored
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.id != "CEL0042"),
        "reporting diagnostics are never persisted; they replay from records",
    );
    let (warm_outcome, warm_text) = check(root.path());
    assert_eq!(warm_outcome, Outcome::DiagnosticsReported, "{warm_text}");
    assert!(warm_text.contains("CEL0042"), "{warm_text}");
}

#[test]
fn editing_a_directive_identifier_is_a_plain_content_miss() {
    // Narrow the directive on a warm cache: the hash moves, the entry
    // is recomputed, and the previously suppressed finding returns
    // while the directive stops being unused.
    let root = project(&[(
        "a.php",
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0018\n",
    )]);
    let (cold, _) = check(root.path());
    assert_eq!(cold, Outcome::Clean);

    std::fs::write(
        root.path().join("a.php"),
        "<?php\nnew MissingOne(); // @celerrate-ignore CEL0019\n",
    )
    .unwrap();
    let (warm, text) = check(root.path());
    assert_eq!(warm, Outcome::DiagnosticsReported, "{text}");
    assert!(text.contains("CEL0018"), "{text}");
    assert!(text.contains("CEL0042"), "{text}");
}

#[test]
fn a_typed_suppression_keeps_its_directive_used_on_the_warm_path() {
    // The directive's only client is a typed-family finding (CEL0030,
    // inside a checked body - top-level code is not a body): the
    // matched attribution comes from the typed half's own records,
    // warm and cold alike - no CEL0042 on either run.
    let source = "<?php\nclass Service { public function boot(): void {} }\nfunction caller(): void {\n    $service = new Service();\n    $service->bot(); // @celerrate-ignore CEL0030\n}\n";
    let root = project(&[("a.php", source)]);
    let (cold, cold_text) = check(root.path());
    assert_eq!(cold, Outcome::Clean, "{cold_text}");
    let (warm, warm_text) = check(root.path());
    assert_eq!(warm, Outcome::Clean, "{warm_text}");
}
