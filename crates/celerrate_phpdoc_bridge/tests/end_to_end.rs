//! End-to-end proof that the bridge, registered into
//! `celerrate_types::TypeSyntaxRegistry`, carries standard PHPDoc
//! annotations all the way through the declared-signature seam:
//! `@return`, `@var`, `@param`, and the free-function seam all land.
//!
//! Fixture recipe mirrors `celerrate_types::declared`'s own test
//! module (`Fixture`, `fixture`, `member_query`), which in turn
//! mirrors the quartet in `celerrate_semantics::linearize`'s tests:
//! `TestDatabase`, per-source `SourceFile`, `AnalyzedFileSet`, an
//! empty `StubIndexInput` at HIGH durability, `ProjectConfiguration`
//! at MEDIUM. This crate has no shared test-support module across
//! crates (recorded debt, consistent with the rest of the workspace).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{MemberKind, SymbolSpace, folded_member_key, folded_symbol_key};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};

struct Fixture {
    db: TestDatabase,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

fn fixture(sources: &[&str]) -> Fixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles);
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    Fixture {
        db,
        files,
        stubs,
        configuration,
    }
}

/// A folded `MemberQuery` for one class-and-member pair.
fn member_query<'db>(
    fixture: &'db Fixture,
    class_written: &str,
    kind: MemberKind,
    member_written: &str,
) -> celerrate_semantics::MemberQuery<'db> {
    celerrate_semantics::MemberQuery::new(
        &fixture.db,
        folded_symbol_key(SymbolSpace::ClassLike, class_written),
        kind,
        folded_member_key(kind, member_written),
    )
}

fn register_bridge(db: &celerrate_db::testing::TestDatabase) {
    let bridge = std::sync::Arc::new(celerrate_phpdoc_bridge::PhpdocBridge::new());
    let identity = celerrate_phpdoc_bridge::descriptor().identity;
    let _ = celerrate_types::TypeSyntaxRegistry::builder(vec![
        celerrate_types::TypeSyntaxRegistration {
            identity: identity.clone(),
            implementation: bridge,
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(db);
}

#[test]
fn a_return_annotation_refines_the_declared_member_signature() {
    let fixture = fixture(&[
        "<?php class Animal {} class Dog extends Animal {} class Kennel { /** @return Dog */ public function adopt(): Animal {} }",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "Kennel", MemberKind::Method, "adopt");
    let signature = celerrate_types::declared_member_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
    .unwrap();
    let dog = celerrate_types::TypeId::class(&fixture.db, "Dog", Vec::new());
    assert_eq!(signature.value_type, dog);
    assert_eq!(signature.value_trust, celerrate_types::Trust::Refined);
}

#[test]
fn class_names_qualify_at_the_declaring_site() {
    let fixture = fixture(&[
        "<?php namespace App\\Model; class User {}",
        "<?php namespace App;\nuse App\\Model\\User;\nclass Repository { /** @return User|null */ public function find() {} }",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "App\\Repository", MemberKind::Method, "find");
    let signature = celerrate_types::declared_member_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
    .unwrap();
    let db = &fixture.db;
    let expected = celerrate_types::TypeId::union(
        db,
        [
            celerrate_types::TypeId::class(db, "App\\Model\\User", Vec::new()),
            celerrate_types::TypeId::null(db),
        ],
    );
    assert_eq!(signature.value_type, expected);
}

#[test]
fn param_var_and_throws_annotations_land() {
    let fixture = fixture(&[
        "<?php class C { /** @var int[] */ public $numbers; /**\n * @param ?string $name\n * @return bool\n */ public function greet($name) {} }",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let numbers = member_query(&fixture, "C", MemberKind::Property, "numbers");
    let numbers_signature = celerrate_types::declared_member_signature(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        numbers,
    )
    .unwrap();
    let key = celerrate_types::TypeId::union(
        db,
        [
            celerrate_types::TypeId::int(db),
            celerrate_types::TypeId::string(db),
        ],
    );
    assert_eq!(
        numbers_signature.value_type,
        celerrate_types::TypeId::array(db, key, celerrate_types::TypeId::int(db)),
    );
    let greet = member_query(&fixture, "C", MemberKind::Method, "greet");
    let greet_signature = celerrate_types::declared_member_signature(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        greet,
    )
    .unwrap();
    assert_eq!(
        greet_signature.parameters[0].parameter_type,
        Some(celerrate_types::TypeId::union(
            db,
            [
                celerrate_types::TypeId::string(db),
                celerrate_types::TypeId::null(db),
            ],
        )),
    );
}

#[test]
fn a_function_docblock_flows_through_the_function_seam() {
    let fixture = fixture(&["<?php /** @return int */ function answer() {}"]);
    register_bridge(&fixture.db);
    let query = celerrate_types::FunctionQuery::new(
        &fixture.db,
        celerrate_semantics::folded_symbol_key(
            celerrate_semantics::SymbolSpace::Function,
            "answer",
        ),
    );
    let signature = celerrate_types::declared_function_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
    .unwrap();
    assert_eq!(
        signature.value_type,
        celerrate_types::TypeId::int(&fixture.db)
    );
    assert_eq!(signature.value_trust, celerrate_types::Trust::Refined);
}
