//! The typed judgments must ride the member boundary's early cutoff: a
//! method-body edit backdates the member tree, so a memoized subtype
//! verdict that consulted the hierarchy does not recompute.

#![allow(clippy::unwrap_used)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{Proof, TypeId, subtype_of};
use salsa::Setter;

#[test]
fn a_body_edit_does_not_recompute_a_hierarchy_verdict() {
    let mut db = TestDatabase::default();
    let parent = SourceFile::new(
        &db,
        FileId::new(0),
        b"<?php class Entity { public function id(): int { return 1; } }".to_vec(),
    );
    let child = SourceFile::new(
        &db,
        FileId::new(1),
        b"<?php class User extends Entity {}".to_vec(),
    );
    let files = AnalyzedFileSet::new(&db, vec![parent, child]);
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);

    let user = TypeId::class(&db, "User", vec![]);
    let entity = TypeId::class(&db, "Entity", vec![]);
    assert_eq!(
        subtype_of(&db, files, stubs, configuration, user, entity),
        Proof::Holds
    );

    db.take_executed();
    parent
        .set_bytes(&mut db)
        .to(b"<?php class Entity { public function id(): int { return 2; } }".to_vec());
    let user = TypeId::class(&db, "User", vec![]);
    let entity = TypeId::class(&db, "Entity", vec![]);
    assert_eq!(
        subtype_of(&db, files, stubs, configuration, user, entity),
        Proof::Holds
    );
    let executed = db.take_executed();
    assert!(
        !executed.iter().any(|query| query.contains("subtype_of")),
        "a body edit must backdate below the judgment, ran: {executed:?}"
    );
}
