//! Invalidation-scope pins for the rule framework's syntax, semantic,
//! and typed-body phases: after each canonical edit class, assert
//! exactly which queries re-executed. The syntax-phase pins are the
//! ones the syntax-version-gating family carried when it lived in
//! `celerrate_semantics` (a version-range change re-filters without
//! re-walking; a per-file edit stays local), re-homed against
//! `syntax_phase_diagnostics` now that the phase owns the diagnostic.
//!
//! The typed-body pins mirror `celerrate_types`' own
//! `body_typed_verdicts` pins (`a_body_edit_rechecks_only_the_editing_body`
//! and `an_edit_above_a_body_reruns_only_the_mapping` in
//! `crates/celerrate_types/tests/invalidation_scope.rs`) against this
//! framework's per-body tier, `body_phase_findings`. They used to run
//! against a fake `TypedBody` rule (`MarkEveryBody`, registered by a
//! `register_typed_fake` helper) because core carried no typed-body
//! family at the time these pins were first written (part 3). That is
//! part 3 history now. Three real typed families
//! (`unknown-members`, `null-dereference`, `argument-checks`) are
//! registered by `core_rules` and, as of the previous task, are the
//! product's serving path, so these two pins measure `register`'s real
//! `core_rules()` composition and a real seeded defect (an unknown
//! method call), exactly like every other pin in this file.
//!
//! The semantic-phase pin below arrived with part 4, once the phase
//! carried its real families (`unknown-symbols`, `symbol-version-gating`)
//! and so something real to invalidate: a same-file body edit that
//! changes reference outcomes re-runs that file's own walk and phase,
//! while every other file's phase backdates behind the unchanged item
//! tree and never runs at all.

// `unwrap`/`expect`/indexing are fine in a test: failing loudly is what
// a test should do.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::DiagnosticId;
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_rules::{
    CORE_IDENTITY_NAME, RuleRegistration, RuleRegistry, Tier, core_rules,
    semantic_phase_diagnostics, syntax_phase_diagnostics, typed_body_phase_diagnostics,
};
use celerrate_semantics::PluginIdentity;
use celerrate_source::{FileId, TextRange, TextSize};
use celerrate_stubs::{StubIndex, StubIndexInput};
use salsa::Setter;

/// Copied per-crate, deliberately not shared: each invalidation-scope
/// suite owns its own execution-log reader (the plan pins one copy per
/// crate, next to the queries it observes).
fn executions_of(log: &[String], query: &str) -> usize {
    let prefix = format!("{query}(");
    log.iter()
        .filter(|entry| entry.contains(prefix.as_str()))
        .count()
}

/// Populates the registry from `core_rules` under the reserved core
/// identity, exactly as the composition root's `register_core_rules`
/// does: this is the framework path the CLI serves.
fn register(db: &TestDatabase) {
    let identity = PluginIdentity {
        name: CORE_IDENTITY_NAME.to_owned(),
        version: "test".to_owned(),
        configuration: String::new(),
    };
    let registrations = core_rules()
        .into_iter()
        .map(|(metadata, implementation)| RuleRegistration {
            identity: identity.clone(),
            active: metadata.tier == Tier::Default,
            metadata,
            implementation,
        })
        .collect();
    let _ = RuleRegistry::builder(registrations)
        .durability(salsa::Durability::HIGH)
        .new(db);
}

/// A configuration spanning `minimum..=8.5`, the version input the phase
/// query reads.
fn configuration_for(db: &TestDatabase, minimum: PhpVersion) -> ProjectConfiguration {
    ProjectConfiguration::builder(PhpVersionRange::new(minimum, PhpVersion::new(8, 5)))
        .durability(salsa::Durability::MEDIUM)
        .new(db)
}

/// The composition pin: `core_rules()` names, in registration order.
/// Dispatch order across every phase is registration order (each phase
/// query drains `registry.registrations(db)` in place, unsorted), so a
/// silent reorder here would silently reorder every phase's diagnostic
/// list wherever two findings tie on the total order otherwise. The
/// two reporting rules join the tail: they run in their own phase, so
/// they never interleave with the six analysis families.
#[test]
fn the_core_rule_set_carries_the_six_migrated_families_and_the_two_reporting_rules() {
    let names: Vec<String> = core_rules()
        .into_iter()
        .map(|(metadata, _)| metadata.name)
        .collect();
    assert_eq!(
        names,
        vec![
            "syntax-version-gating".to_owned(),
            "unknown-symbols".to_owned(),
            "symbol-version-gating".to_owned(),
            "unknown-members".to_owned(),
            "null-dereference".to_owned(),
            "argument-checks".to_owned(),
            "unknown-suppression-identifier".to_owned(),
            "unused-suppression".to_owned(),
        ],
        "registration order is the deterministic dispatch order",
    );
}

#[test]
fn a_version_range_change_reruns_the_phase_but_never_the_walk() {
    // Prime with a gated construct (readonly class, requires 8.2) under
    // minimum 8.1, clear the log, raise the minimum to 8.2, re-query.
    let mut db = TestDatabase::default();
    register(&db);
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php readonly class Point {}".to_vec(),
    );
    let configuration = configuration_for(&db, PhpVersion::new(8, 1));
    assert_eq!(syntax_phase_diagnostics(&db, file, configuration).len(), 1);
    db.take_executed();

    configuration
        .set_php_version_range(&mut db)
        .to(PhpVersionRange::new(
            PhpVersion::new(8, 2),
            PhpVersion::new(8, 5),
        ));
    let diagnostics = syntax_phase_diagnostics(&db, file, configuration);

    assert_eq!(diagnostics, &vec![]);
    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "syntax_phase_diagnostics"),
        1,
        "the configuration is an input of the phase query: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "gated_syntax_uses"),
        0,
        "a version change re-filters without re-walking: {log:?}",
    );
}

#[test]
fn an_edit_to_one_file_reruns_only_its_own_syntax_phase() {
    // Two files, both primed through the phase query; edit file A's bytes
    // (salsa::Setter) to add a second gated construct, re-query both.
    let mut db = TestDatabase::default();
    register(&db);
    let file_a = SourceFile::new(&db, FileId::new(0), b"<?php readonly class A {}".to_vec());
    let file_b = SourceFile::new(&db, FileId::new(1), b"<?php readonly class B {}".to_vec());
    let configuration = configuration_for(&db, PhpVersion::new(8, 1));
    assert_eq!(
        syntax_phase_diagnostics(&db, file_a, configuration).len(),
        1
    );
    assert_eq!(
        syntax_phase_diagnostics(&db, file_b, configuration).len(),
        1
    );
    db.take_executed();

    file_a
        .set_bytes(&mut db)
        .to(b"<?php readonly class A {} readonly class C {}".to_vec());
    assert_eq!(
        syntax_phase_diagnostics(&db, file_a, configuration).len(),
        2
    );
    let _ = syntax_phase_diagnostics(&db, file_b, configuration);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "syntax_phase_diagnostics"),
        1,
        "file B's phase is untouched by file A's edit: {log:?}",
    );
}

/// The seeded defect the two typed-body pins below share: `User::save`
/// exists, `first` misspells the call as `svae` (CEL0030, the
/// `unknown-members` family), and `second` calls it correctly. One
/// class body plus two function bodies means three bodies for the
/// per-body tier to enumerate; the prime below reads that count off
/// the query's own return value rather than asserting it, since the
/// class's method is a body too.
const SEEDED_TYPED_DEFECT: &[u8] = b"<?php\nclass User { public function save(): void {} }\nfunction first(User $u) { $u->svae(); }\nfunction second(User $u) { $u->save(); }\n";

/// Mirrors `celerrate_types`' `a_body_edit_rechecks_only_the_editing_body`
/// against this framework's own per-body tier, now driven by `register`'s
/// real `core_rules()` (the fake `MarkEveryBody` rule this pin used to
/// need is part 3 history): three bodies primed through
/// `typed_body_phase_diagnostics`, a statement appended inside
/// `second`'s body, then a re-query. `first`'s `body_phase_findings`
/// memo is keyed on its own unedited `body_ir` and carries no
/// dependency edge into `second`'s edit at all, so only the editing
/// body's tier re-executes; `body_typed_verdicts`, the per-body walk the
/// typed families share underneath the tier (`celerrate_types`), is
/// only ever entered once per body per revision regardless of how many
/// active typed families read it, so it re-executes exactly once too.
#[test]
fn a_body_edit_reruns_only_the_editing_bodys_phase() {
    let mut db = TestDatabase::default();
    register(&db);
    let file = SourceFile::new(&db, FileId::new(0), SEEDED_TYPED_DEFECT.to_vec());
    let files = AnalyzedFileSet::new(&db, vec![file]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = configuration_for(&db, PhpVersion::new(8, 1));
    let first = typed_body_phase_diagnostics(&db, file, files, stubs, configuration);
    assert_eq!(
        first.len(),
        1,
        "only the misspelled call in `first` reports: {first:?}",
    );
    assert_eq!(first[0].id, DiagnosticId::new("CEL0030"));
    db.take_executed();

    file.set_bytes(&mut db).to(
        b"<?php\nclass User { public function save(): void {} }\nfunction first(User $u) { $u->svae(); }\nfunction second(User $u) { $u->save(); echo 1; }\n"
            .to_vec(),
    );
    let second = typed_body_phase_diagnostics(&db, file, files, stubs, configuration);
    assert_eq!(second.len(), 1, "the same single defect still reports");

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "body_phase_findings"),
        1,
        "editing one body never re-checks its siblings: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "body_typed_verdicts"),
        1,
        "the real walk shares this tier with celerrate_types; only the \
         editing body's memo re-runs: {log:?}",
    );
}

/// Mirrors `celerrate_types`' `an_edit_above_a_body_reruns_only_the_mapping`
/// against this framework's own per-body tier, now driven by `register`'s
/// real `core_rules()`: a comment line prepended above every body shifts
/// every subsequent offset without changing the parsed structure at
/// all. If findings were keyed by `TextRange` anywhere above the
/// reconciliation tail, this edit would force every body's
/// `body_phase_findings` (and the `body_typed_verdicts` walk beneath
/// it) to re-run; the design's claim is the opposite. The finding is
/// anchored by declaration identity, range-free, so it backdates under
/// the shift and only `resolved_diagnostic`'s mapping work (through the
/// aggregate query) redoes anything, the reported diagnostic moving to
/// its new location.
#[test]
fn an_edit_above_a_body_reruns_no_body_phase() {
    let mut db = TestDatabase::default();
    register(&db);
    let file = SourceFile::new(&db, FileId::new(0), SEEDED_TYPED_DEFECT.to_vec());
    let files = AnalyzedFileSet::new(&db, vec![file]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = configuration_for(&db, PhpVersion::new(8, 1));
    let first = typed_body_phase_diagnostics(&db, file, files, stubs, configuration);
    assert_eq!(
        first.len(),
        1,
        "only the misspelled call in `first` reports: {first:?}",
    );
    // Capture the pre-edit ranges as owned values: `first` is a `&Vec`
    // borrowed from the salsa memo (`returns(ref)`), and the upcoming
    // edit invalidates it, so the ranges must survive the re-query on
    // their own.
    let pre_edit_ranges: Vec<TextRange> = first
        .iter()
        .filter_map(|diagnostic| diagnostic.span().map(|(_, range)| range))
        .collect();
    assert_eq!(
        pre_edit_ranges.len(),
        1,
        "the finding resolves to a concrete span before the edit",
    );
    db.take_executed();

    let leading_comment = "// a comment line\n";
    let delta = TextSize::from(
        u32::try_from(leading_comment.len()).expect("the comment line fits in a u32 offset"),
    );
    file.set_bytes(&mut db).to(
        b"<?php\n// a comment line\nclass User { public function save(): void {} }\nfunction first(User $u) { $u->svae(); }\nfunction second(User $u) { $u->save(); }\n"
            .to_vec(),
    );
    let second = typed_body_phase_diagnostics(&db, file, files, stubs, configuration);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "body_phase_findings"),
        0,
        "range-free findings backdate under an offset shift: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "body_typed_verdicts"),
        0,
        "the shared per-body walk backdates too, for the same reason: {log:?}",
    );
    assert_eq!(second.len(), 1, "the diagnostic moved with its range");
    let post_edit_ranges: Vec<TextRange> = second
        .iter()
        .filter_map(|diagnostic| diagnostic.span().map(|(_, range)| range))
        .collect();
    // Both lists come out of `typed_body_phase_diagnostics`, which sorts
    // by anchor before returning; a uniform shift never reorders spans,
    // so pairing by index is exact, not merely convenient. A stale-range
    // reconciliation bug (post-edit ranges left at their pre-edit
    // offsets) would make this fail, since `pre + delta != pre` for a
    // non-zero shift.
    assert_eq!(
        post_edit_ranges,
        pre_edit_ranges
            .iter()
            .map(|range| *range + delta)
            .collect::<Vec<_>>(),
        "each diagnostic's range should have shifted by the prepended \
         comment's byte length, not stayed at its pre-edit location",
    );
}

#[test]
fn a_body_edit_reruns_its_own_semantic_walk_and_never_anothers_phase() {
    let mut db = TestDatabase::default();
    register(&db);
    let library = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php namespace Lib; class Helper { public function go(): void {} }".to_vec(),
    );
    let consumer = SourceFile::new(
        &db,
        FileId::new(1),
        b"<?php namespace App; use Lib\\Helper; $x = new Helper(); $y = new Missing();".to_vec(),
    );
    let files = AnalyzedFileSet::new(&db, vec![library, consumer]);
    let stubs = StubIndexInput::builder(StubIndex::default())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = configuration_for(&db, PhpVersion::new(8, 1));
    assert!(semantic_phase_diagnostics(&db, library, files, stubs, configuration).is_empty());
    assert_eq!(
        semantic_phase_diagnostics(&db, consumer, files, stubs, configuration).len(),
        1,
        "the consumer's unknown class reports",
    );
    db.take_executed();

    // A body edit that introduces an unresolved reference inside the
    // library's method: the item tree is unchanged, so the consumer's
    // resolutions backdate behind the unchanged symbol table and its
    // phase never runs; the library's own per-file walk honestly
    // re-runs (the design's stated same-file behavior), its outcomes
    // change, and its phase re-runs over them.
    library.set_bytes(&mut db).to(
        b"<?php namespace Lib; class Helper { public function go(): void { new Ghost(); } }"
            .to_vec(),
    );
    assert_eq!(
        semantic_phase_diagnostics(&db, library, files, stubs, configuration).len(),
        1,
        "the library's new unknown class reports",
    );
    assert_eq!(
        semantic_phase_diagnostics(&db, consumer, files, stubs, configuration).len(),
        1,
    );

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "reference_outcomes"),
        1,
        "only the edited file's walk re-runs: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "semantic_phase_diagnostics"),
        1,
        "the library's phase re-runs over its changed outcomes; the \
         consumer's backdates whole: {log:?}",
    );
}
