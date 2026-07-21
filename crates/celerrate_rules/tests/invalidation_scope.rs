//! Invalidation-scope pins for the rule framework's syntax and
//! typed-body phases: after each canonical edit class, assert exactly
//! which queries re-executed. The syntax-phase pins are the ones the
//! syntax-version-gating family carried when it lived in
//! `celerrate_semantics` (a version-range change re-filters without
//! re-walking; a per-file edit stays local), re-homed against
//! `syntax_phase_diagnostics` now that the phase owns the diagnostic.
//!
//! The typed-body pins mirror `celerrate_types`' own
//! `body_typed_verdicts` pins (`a_body_edit_rechecks_only_the_editing_body`
//! and `an_edit_above_a_body_reruns_only_the_mapping` in
//! `crates/celerrate_types/tests/invalidation_scope.rs`) against this
//! framework's per-body tier, `body_phase_findings`, with a fake
//! `TypedBody` rule (`MarkEveryBody`) registered so the tier has
//! something to invalidate.
//!
//! No semantic-phase pin lives here yet: the skeleton's fake semantic
//! rule (`EmitPerFile`, in `phases.rs`'s own tests) reads no file
//! content at all, so a cross-file invalidation pin against it would
//! measure the fake, not the framework. The semantic phase's real
//! invalidation pins arrive in part 4, once it carries its real
//! families and something to actually invalidate.

// `unwrap`/`expect`/indexing are fine in a test: failing loudly is what
// a test should do.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::sync::Arc;

use celerrate_db::SourceFile;
use celerrate_db::testing::TestDatabase;
use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_rules::{
    CORE_IDENTITY_NAME, FindingAnchor, FindingSink, RuleGroup, RuleIdentifier, RuleImplementation,
    RuleMetadata, RuleRegistration, RuleRegistry, Tier, TypedBodyRule, core_rules,
    syntax_phase_diagnostics, typed_body_phase_diagnostics,
};
use celerrate_semantics::PluginIdentity;
use celerrate_source::{FileId, TextRange, TextSize};
use celerrate_types::TypedBodyContext;
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

/// A fake typed-body rule that marks every body it sees, by declaration
/// anchor. The two pins below need something for the per-body tier to
/// invalidate; the core rule set carries no typed-body family yet
/// (part 4 migrates the first one onto this framework).
struct MarkEveryBody;

impl TypedBodyRule for MarkEveryBody {
    fn check(&self, context: &TypedBodyContext<'_>, sink: &mut FindingSink<'_>) {
        sink.report(
            DiagnosticId::new("CEL9996"),
            FindingAnchor::Declaration(context.body()),
            "marked body".to_owned(),
        );
    }
}

/// Registers only the fake `MarkEveryBody` rule, under a throwaway
/// plugin identity, replacing `register`'s use of `core_rules` for the
/// two typed-body pins below.
fn register_typed_fake(db: &TestDatabase) {
    let registration = RuleRegistration {
        identity: PluginIdentity {
            name: "test-plugin".to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        },
        active: true,
        metadata: RuleMetadata {
            name: "mark-every-body".to_owned(),
            group: RuleGroup::Correctness,
            identifiers: vec![RuleIdentifier {
                id: DiagnosticId::new("CEL9996"),
                severity: Severity::Error,
            }],
            tier: Tier::Default,
        },
        implementation: RuleImplementation::TypedBody(Arc::new(MarkEveryBody)),
    };
    let _ = RuleRegistry::builder(vec![registration])
        .durability(salsa::Durability::HIGH)
        .new(db);
}

/// Mirrors `celerrate_types`' `a_body_edit_rechecks_only_the_editing_body`
/// against this framework's own per-body tier: two bodies primed through
/// `typed_body_phase_diagnostics`, a statement appended inside the
/// second one, then a re-query. `first`'s `body_phase_findings` memo is
/// keyed on its own unedited `body_ir` and carries no dependency edge
/// into `second`'s edit at all, so only the editing body's tier
/// re-executes.
#[test]
fn a_body_edit_reruns_only_the_editing_bodys_phase() {
    let mut db = TestDatabase::default();
    register_typed_fake(&db);
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php\nfunction first() { echo 1; }\nfunction second() { echo 2; }\n".to_vec(),
    );
    assert_eq!(typed_body_phase_diagnostics(&db, file).len(), 2);
    db.take_executed();

    file.set_bytes(&mut db).to(
        b"<?php\nfunction first() { echo 1; }\nfunction second() { echo 2; echo 3; }\n".to_vec(),
    );
    let second = typed_body_phase_diagnostics(&db, file);
    assert_eq!(second.len(), 2, "both bodies still report");

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "body_phase_findings"),
        1,
        "editing one body never re-checks its siblings: {log:?}",
    );
}

/// Mirrors `celerrate_types`' `an_edit_above_a_body_reruns_only_the_mapping`
/// against this framework's own per-body tier: a comment line prepended
/// above every body shifts every subsequent offset without changing the
/// parsed structure at all. If findings were keyed by `TextRange`
/// anywhere above the reconciliation tail, this edit would force every
/// body's `body_phase_findings` to re-run; the design's claim is the
/// opposite — the finding is anchored by declaration identity,
/// range-free, so it backdates under the shift and only
/// `resolved_diagnostic`'s mapping work (through the aggregate query)
/// redoes anything, the reported diagnostics moving to their new
/// locations.
#[test]
fn an_edit_above_a_body_reruns_no_body_phase() {
    let mut db = TestDatabase::default();
    register_typed_fake(&db);
    let file = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php\nfunction first() { echo 1; }\nfunction second() { echo 2; }\n".to_vec(),
    );
    let first = typed_body_phase_diagnostics(&db, file);
    assert_eq!(first.len(), 2);
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
        2,
        "both findings resolve to a concrete span before the edit",
    );
    db.take_executed();

    let leading_comment = "// a comment line\n";
    let delta = TextSize::from(
        u32::try_from(leading_comment.len()).expect("the comment line fits in a u32 offset"),
    );
    file.set_bytes(&mut db).to(
        b"<?php\n// a comment line\nfunction first() { echo 1; }\nfunction second() { echo 2; }\n"
            .to_vec(),
    );
    let second = typed_body_phase_diagnostics(&db, file);

    let log = db.take_executed();
    assert_eq!(
        executions_of(&log, "body_phase_findings"),
        0,
        "range-free findings backdate under an offset shift: {log:?}",
    );
    assert_eq!(second.len(), 2, "the diagnostics moved with their ranges");
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
