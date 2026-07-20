//! Invalidation-scope pins for the rule framework's syntax phase: after
//! each canonical edit class, assert exactly which queries re-executed.
//! These are the pins the syntax-version-gating family carried when it
//! lived in `celerrate_semantics` (a version-range change re-filters
//! without re-walking; a per-file edit stays local), re-homed against
//! `syntax_phase_diagnostics` now that the phase owns the diagnostic.

// `unwrap`/`expect`/indexing are fine in a test: failing loudly is what
// a test should do.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_db::SourceFile;
use celerrate_db::testing::TestDatabase;
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_rules::{
    CORE_IDENTITY_NAME, RuleRegistration, RuleRegistry, Tier, core_rules, syntax_phase_diagnostics,
};
use celerrate_semantics::PluginIdentity;
use celerrate_source::FileId;
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
