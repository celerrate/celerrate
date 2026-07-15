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
            implementation: bridge.clone(),
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(db);
    let _ = celerrate_semantics::VirtualSymbolRegistry::builder(vec![
        celerrate_semantics::VirtualSymbolRegistration {
            identity,
            provider: bridge,
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
        "<?php class C { /** @var int[] */ public $numbers; /**\n * @param ?string $name\n * @return bool\n * @throws \\RuntimeException\n */ public function greet($name) {} }",
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
    let annotations = celerrate_types::member_annotations(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        greet,
    );
    assert_eq!(
        annotations.throws,
        vec![celerrate_types::TypeId::class(
            db,
            "RuntimeException",
            Vec::new()
        )],
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

#[test]
fn a_property_annotation_declares_a_member_that_exists_and_types() {
    let fixture = fixture(&["<?php /** @property string $title */ class Post {}"]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "Post", MemberKind::Property, "title");
    let resolution = celerrate_semantics::lookup_member(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    );
    assert!(matches!(
        resolution,
        Some(celerrate_semantics::MemberResolution::Virtual { .. }),
    ));
    let signature = celerrate_types::declared_member_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
    .unwrap();
    assert_eq!(
        signature.value_type,
        celerrate_types::TypeId::string(&fixture.db)
    );
    assert_eq!(signature.value_trust, celerrate_types::Trust::Refined);
}

#[test]
fn a_method_annotation_declares_a_typed_virtual_method() {
    let fixture = fixture(&[
        "<?php class User {} /** @method static User find(int $id) */ class Repository {}",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "Repository", MemberKind::Method, "find");
    let signature = celerrate_types::declared_member_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
    .unwrap();
    assert_eq!(
        signature.value_type,
        celerrate_types::TypeId::class(&fixture.db, "User", Vec::new()),
    );
    assert_eq!(signature.parameters.len(), 1);
    assert_eq!(
        signature.parameters[0].parameter_type,
        Some(celerrate_types::TypeId::int(&fixture.db)),
    );
}

#[test]
fn dialect_atoms_and_generics_lower_through_the_table() {
    let fixture = fixture(&[
        "<?php class C {\n\
         /** @return array<int, string> */ public function a() {}\n\
         /** @return positive-int */ public function b() {}\n\
         /** @return 'yes'|'no' */ public function c() {}\n\
         /** @return int<1, max> */ public function d() {}\n\
         /** @return class-string<\\App\\User> */ public function e() {}\n\
         /** @return array-key */ public function f() {}\n\
         /** @return non-empty-list<string> */ public function g() {}\n\
         /** @return iterable<User> */ public function h() {}\n\
         }",
        "<?php class User {}",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let value = |name: &str| {
        let query = member_query(&fixture, "C", MemberKind::Method, name);
        celerrate_types::declared_member_signature(
            db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap()
        .value_type
    };
    use celerrate_types::TypeId;
    assert_eq!(
        value("a"),
        TypeId::array(db, TypeId::int(db), TypeId::string(db)),
    );
    assert_eq!(value("b"), TypeId::int_range(db, Some(1), None));
    assert_eq!(
        value("c"),
        TypeId::union(
            db,
            [
                TypeId::string_literal(db, "yes"),
                TypeId::string_literal(db, "no"),
            ]
        ),
    );
    assert_eq!(value("d"), TypeId::int_range(db, Some(1), None));
    assert_eq!(
        value("e"),
        TypeId::class_string(db, Some(TypeId::class(db, "App\\User", Vec::new()))),
    );
    assert_eq!(
        value("f"),
        TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
    );
    assert_eq!(value("g"), TypeId::non_empty_list(db, TypeId::string(db)),);
    assert_eq!(
        value("h"),
        TypeId::iterable(db, TypeId::mixed(db), TypeId::class(db, "User", Vec::new())),
    );
}

#[test]
fn shapes_callables_and_the_documented_widenings_lower() {
    let fixture = fixture(&["<?php class C {\n\
         /** @return array{id: int, name?: string} */ public function a() {}\n\
         /** @return array{id: int, ...} */ public function b() {}\n\
         /** @return array{id: int, ...<string, bool>} */ public function c() {}\n\
         /** @return object{a: int} */ public function d() {}\n\
         /** @return callable(int, string=): bool */ public function e() {}\n\
         /** @return $this */ public function f() {}\n\
         /** @return Foo::BAR */ public function g() {}\n\
         /** @return ($flags is 1 ? string : bool) */ public function h() {}\n\
         /** @return \\Closure<T of Mode>(T): T */ public function i() {}\n\
         }"]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let value = |name: &str| {
        let query = member_query(&fixture, "C", MemberKind::Method, name);
        celerrate_types::declared_member_signature(
            db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap()
        .value_type
    };
    use celerrate_types::{CallableParameter, ShapeField, ShapeKey, TypeId};
    let array_key = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
    assert_eq!(
        value("a"),
        TypeId::shape(
            db,
            vec![
                ShapeField {
                    key: ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::int(db),
                },
                ShapeField {
                    key: ShapeKey::String("name".to_owned()),
                    optional: true,
                    value: TypeId::string(db),
                },
            ]
        ),
    );
    // Unsealed shapes give up their field knowledge: the documented
    // widening is the general (non-empty when a field is required)
    // array — a supertype, never a truncation into wrongness.
    assert_eq!(
        value("b"),
        TypeId::non_empty_array(db, array_key, TypeId::mixed(db)),
    );
    assert_eq!(
        value("c"),
        TypeId::non_empty_array(
            db,
            array_key,
            TypeId::union(db, [TypeId::int(db), TypeId::bool(db)]),
        ),
    );
    // No object-shape lattice form: `object` is the widening.
    assert_eq!(value("d"), TypeId::object(db));
    assert_eq!(
        value("e"),
        TypeId::callable(
            db,
            vec![
                CallableParameter {
                    parameter_type: TypeId::int(db),
                    optional: false,
                    variadic: false,
                    by_reference: false,
                },
                CallableParameter {
                    parameter_type: TypeId::string(db),
                    optional: true,
                    variadic: false,
                    by_reference: false,
                },
            ],
            TypeId::bool(db),
        ),
    );
    // `@return $this` collapses into `static` (design section 3).
    assert_eq!(value("f"), TypeId::static_placeholder(db));
    // Constant fetches await member facts: `mixed`, documented.
    assert_eq!(value("g"), TypeId::mixed(db));
    // Parameter-subject conditionals: the undecided branch union.
    assert_eq!(
        value("h"),
        TypeId::union(db, [TypeId::string(db), TypeId::bool(db)]),
    );
    // Callable-scoped templates lower their occurrences to `mixed`.
    assert_eq!(
        value("i"),
        TypeId::callable(
            db,
            vec![CallableParameter {
                parameter_type: TypeId::mixed(db),
                optional: false,
                variadic: false,
                by_reference: false,
            }],
            TypeId::mixed(db),
        ),
    );
}
