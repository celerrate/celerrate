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
use celerrate_cli::cache::verdict::{TypedOutcome, VerdictLookup, lookup_verdict};
use celerrate_cli::session::Session;
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

fn check(root: &Path) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "check".into(), root.as_os_str().into()],
        &mut output,
        ColorMode::Plain,
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

#[test]
fn a_partial_hit_keeps_a_typed_only_directive_used() {
    // The genuine partial hit the matrix wanted pinned: untyped
    // SERVED, typed RECOMPUTED, on the very file that carries the
    // directive.
    //
    // Two files. `src/Consumer.php` carries the directive over the
    // same kind of finding as
    // `a_typed_suppression_keeps_its_directive_used_on_the_warm_path`
    // above (CEL0030, a typed-only finding: `retain_unsuppressed` on
    // the untyped stream never sees it, so the directive's only
    // admitting record is typed). `src/Service.php` declares the
    // `Service` class `Consumer.php` calls into. Between the two runs
    // `Service.php` gains a new public method and `Consumer.php` stays
    // byte-identical.
    //
    // That split matters because the untyped half and the typed half
    // validate against different evidence (`crates/celerrate_cli/src/
    // cache/verdict.rs`): the untyped half revalidates each
    // `ResolutionRecord`'s `ResolutionAnswer`, which for a `Source`
    // symbol is reduced to the unit case and carries no member
    // information at all, so `Service` gaining a method leaves every
    // one of `Consumer.php`'s stored records matching and its content
    // hash unmoved - the untyped half is SERVED. The typed half
    // instead revalidates each consulted class's `class_surface_digest`,
    // which folds in the full linearized member list
    // (`crates/celerrate_types/src/records.rs`), so the same edit moves
    // it and forces `TypedOutcome::Recompute` - the typed half is
    // RECOMPUTED. This is exactly `cross_file_source_answers_replay_
    // equal`'s technique in `tests/cache_equivalence.rs`, reused to
    // move a digest rather than an answer.
    let service_v1 = "<?php\nclass Service\n{\n    public function boot(): void {}\n}\n";
    let service_v2 = "<?php\nclass Service\n{\n    public function boot(): void {}\n\n    public function extra(): void {}\n}\n";
    let consumer = "<?php\nfunction caller(): void\n{\n    $service = new Service();\n    $service->bot(); // @celerrate-ignore CEL0030\n}\n";

    let root = project(&[
        ("src/Service.php", service_v1),
        ("src/Consumer.php", consumer),
    ]);
    let (cold, cold_text) = check(root.path());
    assert_eq!(cold, Outcome::Clean, "{cold_text}");

    // Only Service.php moves; Consumer.php, the directive-carrying
    // file, is never touched.
    std::fs::write(root.path().join("src/Service.php"), service_v2).unwrap();

    // A read-only inspection session, opened after the edit but before
    // the actual warm `check()` run below: it loads Service.php's new
    // bytes and Consumer.php's unchanged ones, then asks the cache
    // directly what it would serve for Consumer.php, without itself
    // persisting anything that could paper over a wrong answer before
    // it is observed.
    let session = Session::start(root.path());
    let inputs = session.inputs();
    let Some(&consumer_file) = session.sources.iter().find_map(|(&id, file)| {
        session
            .vfs
            .path(id)
            .filter(|path| path.ends_with("Consumer.php"))
            .map(|_| file)
    }) else {
        panic!("Consumer.php must be among the analyzed sources");
    };

    let VerdictLookup::Hit { typed, .. } = lookup_verdict(&inputs, consumer_file) else {
        panic!(
            "the untyped half must still validate: only Service.php changed, \
             and a resolution record's answer carries no member information",
        );
    };
    assert_eq!(
        typed,
        TypedOutcome::Recompute,
        "the typed half must recompute: Service's surface digest moved \
         when it gained a public method",
    );

    // The actual warm run: a fresh session that analyzes and persists.
    // The union must still be honest - the directive's only admitting
    // record is typed, and the typed half just recomputed, so this is
    // the one place a dropped `matched_typed` contribution would show
    // up as a returned CEL0042.
    let (warm, warm_text) = check(root.path());
    assert_eq!(warm, Outcome::Clean, "{warm_text}");
}
