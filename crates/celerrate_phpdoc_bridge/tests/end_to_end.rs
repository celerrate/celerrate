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

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

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

/// A projection of `ParsedAnnotations`'s new fields, captured
/// as owned data so it outlives the `'db`-scoped call that produced
/// it. `MemberAnnotations` (the tracked wrapper `member_annotations`
/// returns) does not forward `templates`/`ancestors`/`variables` yet:
/// that wiring has not landed, so this recording wrapper is the only way
/// to observe them, by capturing them as a side effect of triggering
/// the real seam.
#[derive(Debug, Default, Clone)]
struct Captured {
    /// One entry per lowered ancestor: its qualified, folded class
    /// name and its fixed generic argument count.
    ancestors: Vec<(String, usize)>,
    /// `@template` declaration names, in declaration order.
    template_names: Vec<String>,
    /// Each `@template`'s lowered bound, `display`-rendered, in the
    /// same order as `template_names`; `None` when the template
    /// declares no bound (the bound lowering was otherwise unpinned by
    /// any test).
    template_bounds: Vec<Option<String>>,
    /// Named inline `@var` entries' variable names.
    variable_names: Vec<String>,
}

/// Wraps the real [`celerrate_phpdoc_bridge::PhpdocBridge`], delegates
/// every call to it unchanged, and records a projection of what it
/// returns for EVERY `parse_docblock` call, keyed by the call's raw
/// docblock text: a single overwritten slot
/// would silently read whichever docblock parsed last if a fixture
/// ever carried both a class-level and a member-level docblock.
/// Registering this instead of the bare bridge lets a test observe
/// `ParsedAnnotations`'s new fields through the real
/// extraction-and-lowering pipeline (dialect classification, tier
/// resolution, site qualification) and the real production seam
/// (`celerrate_types::member_annotations`), without needing direct
/// access to `AnnotationSite`'s private constructor.
struct RecordingBridge {
    inner: celerrate_phpdoc_bridge::PhpdocBridge,
    parses: std::sync::Mutex<Vec<(String, Captured)>>,
}

impl celerrate_types::TypeSyntax for RecordingBridge {
    fn can_parse(&self, docblock: &str) -> bool {
        self.inner.can_parse(docblock)
    }

    fn parse_docblock<'db>(
        &self,
        site: &celerrate_types::AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> celerrate_types::ParsedAnnotations<'db> {
        let parsed = self.inner.parse_docblock(site, docblock);
        let context = site.types();
        let captured = Captured {
            ancestors: parsed
                .ancestors
                .iter()
                .map(|ancestor| (ancestor.class_name.clone(), ancestor.arguments.len()))
                .collect(),
            template_names: parsed
                .templates
                .iter()
                .map(|template| template.name.clone())
                .collect(),
            template_bounds: parsed
                .templates
                .iter()
                .map(|template| template.bound.map(|bound| context.display(bound)))
                .collect(),
            variable_names: parsed
                .variables
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        };
        self.parses
            .lock()
            .unwrap()
            .push((docblock.to_owned(), captured));
        parsed
    }

    fn parse_type_expression<'db>(
        &self,
        site: &celerrate_types::AnnotationSite<'db, '_>,
        expression: &str,
    ) -> Option<celerrate_types::TypeId<'db>> {
        self.inner.parse_type_expression(site, expression)
    }
}

/// Registers a [`RecordingBridge`] in place of the bare bridge, and
/// hands back the `Arc` so the test can read its recorded parses after
/// triggering the seam.
fn register_recording_bridge(
    db: &celerrate_db::testing::TestDatabase,
) -> std::sync::Arc<RecordingBridge> {
    let recording = std::sync::Arc::new(RecordingBridge {
        inner: celerrate_phpdoc_bridge::PhpdocBridge::new(),
        parses: std::sync::Mutex::new(Vec::new()),
    });
    let identity = celerrate_phpdoc_bridge::descriptor().identity;
    let _ = celerrate_types::TypeSyntaxRegistry::builder(vec![
        celerrate_types::TypeSyntaxRegistration {
            identity,
            implementation: recording.clone(),
        },
    ])
    .durability(salsa::Durability::HIGH)
    .new(db);
    recording
}

/// Finds the captured projection for the one recorded parse whose raw
/// docblock text contains `needle`: lets a
/// test select the parse it means by content instead of assuming
/// there is exactly one `parse_docblock` call, or that it is the last
/// one.
fn captured_containing(recording: &RecordingBridge, needle: &str) -> Captured {
    recording
        .parses
        .lock()
        .unwrap()
        .iter()
        .find(|(docblock, _)| docblock.contains(needle))
        .unwrap_or_else(|| panic!("no recorded parse contains {needle:?}"))
        .1
        .clone()
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
    // `@return $this` collapses into `static`.
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

#[test]
fn tool_prefixed_precedence_holds_through_the_seam() {
    let fixture = fixture(&[
        "<?php class C { /**\n * @return string\n * @phpstan-return int\n */ public function pick() {} }",
    ]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "C", MemberKind::Method, "pick");
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
        celerrate_types::TypeId::int(&fixture.db)
    );
}

#[test]
fn template_variables_resolve_through_the_annotation_scope() {
    let fixture = fixture(&[
        "<?php /** @template T of \\Entity */ class Repository {\n\
         /** @return T */ public function find() {}\n\
         /** @template U\n * @return U\n */ public function pluck() {}\n\
         }",
        "<?php class Entity {}",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let value = |name: &str| {
        let query = member_query(&fixture, "Repository", MemberKind::Method, name);
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
    let class_scope = folded_symbol_key(SymbolSpace::ClassLike, "Repository");
    // Class-level templates reach member docblocks, keyed at the
    // class scope with their bound lowered.
    assert_eq!(
        value("find"),
        celerrate_types::TypeId::template(
            db,
            &class_scope,
            "T",
            celerrate_types::TypeId::class(db, "Entity", Vec::new()),
        ),
    );
    // Member-level templates key at `<class key>::<member key>` and
    // default their bound to `mixed`.
    let member_scope = format!(
        "{class_scope}::{}",
        folded_member_key(MemberKind::Method, "pluck"),
    );
    assert_eq!(
        value("pluck"),
        celerrate_types::TypeId::template(
            db,
            &member_scope,
            "U",
            celerrate_types::TypeId::mixed(db),
        ),
    );
}

#[test]
fn member_templates_shadow_class_templates_and_virtual_payloads_see_the_class_scope() {
    let fixture = fixture(&[
        "<?php class A {} class B {}\n\
         /** @template T of A */ class Box {\n\
         /** @template T of B\n * @return T\n */ public function shadowed() {}\n\
         }",
        "<?php /** @template T\n * @property list<T> $items\n */ class Bag {}",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let box_scope = folded_symbol_key(SymbolSpace::ClassLike, "Box");
    let shadowed = member_query(&fixture, "Box", MemberKind::Method, "shadowed");
    let signature = celerrate_types::declared_member_signature(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        shadowed,
    )
    .unwrap();
    let member_scope = format!(
        "{box_scope}::{}",
        folded_member_key(MemberKind::Method, "shadowed"),
    );
    assert_eq!(
        signature.value_type,
        celerrate_types::TypeId::template(
            db,
            &member_scope,
            "T",
            celerrate_types::TypeId::class(db, "B", Vec::new()),
        ),
    );
    // A virtual member's payload resolves class-level templates.
    let items = member_query(&fixture, "Bag", MemberKind::Property, "items");
    let signature = celerrate_types::declared_member_signature(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        items,
    )
    .unwrap();
    let bag_scope = folded_symbol_key(SymbolSpace::ClassLike, "Bag");
    assert_eq!(
        signature.value_type,
        celerrate_types::TypeId::list(
            db,
            celerrate_types::TypeId::template(
                db,
                &bag_scope,
                "T",
                celerrate_types::TypeId::mixed(db),
            ),
        ),
    );
}

#[test]
fn a_variance_marked_template_still_declares_and_a_template_conditional_lowers() {
    let fixture = fixture(&["<?php /** @template-covariant T */ class Producer {\n\
         /** @return T */ public function produce() {}\n\
         /** @return (T is string ? int : bool) */ public function branch() {}\n\
         }"]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let scope = folded_symbol_key(SymbolSpace::ClassLike, "Producer");
    let template =
        celerrate_types::TypeId::template(db, &scope, "T", celerrate_types::TypeId::mixed(db));
    let value = |name: &str| {
        let query = member_query(&fixture, "Producer", MemberKind::Method, name);
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
    assert_eq!(value("produce"), template);
    assert_eq!(
        value("branch"),
        celerrate_types::TypeId::conditional(
            db,
            template,
            celerrate_types::TypeId::string(db),
            celerrate_types::TypeId::int(db),
            celerrate_types::TypeId::bool(db),
            false,
        ),
    );
}

#[test]
fn assertions_are_carried_through_the_annotation_seam() {
    // The webmozart/assert pattern.
    let fixture = fixture(&[
        "<?php class Assert { /** @psalm-assert string $value */ public static function string($value) {} }",
    ]);
    register_bridge(&fixture.db);
    let db = &fixture.db;
    let query = member_query(&fixture, "Assert", MemberKind::Method, "string");
    let annotations = celerrate_types::member_annotations(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    );
    assert_eq!(
        annotations.assertions,
        vec![celerrate_types::ParsedAssertion::new(
            "$value".to_owned(),
            celerrate_types::TypeId::string(db),
            celerrate_types::AssertionPolarity::Always,
            false,
        )],
    );
}

#[test]
fn ancestors_lower_qualified_with_their_argument_types() {
    // Namespace `App`, a `use Doctrine\Repo as Base;` import in scope:
    // `class_name` must arrive fully qualified and folded, and the
    // fixed generic argument must survive.
    let fixture = fixture(&[
        "<?php namespace App;\nuse Doctrine\\Repo as Base;\nclass C { /** @extends Base<User> */ public function noop() {} }",
    ]);
    let recording = register_recording_bridge(&fixture.db);
    let query = member_query(&fixture, "App\\C", MemberKind::Method, "noop");
    let _ = celerrate_types::member_annotations(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    );
    let captured = captured_containing(&recording, "@extends");
    assert_eq!(
        captured.ancestors,
        vec![("doctrine\\repo".to_owned(), 1)],
        "qualified and folded",
    );
}

#[test]
fn templates_lower_in_declaration_order_with_their_bounds() {
    let fixture = fixture(&["<?php class C {\n\
         /**\n\
          * @template TKey of int\n\
          * @template TValue\n\
          */\n\
         public function noop() {}\n\
         }"]);
    let recording = register_recording_bridge(&fixture.db);
    let query = member_query(&fixture, "C", MemberKind::Method, "noop");
    let _ = celerrate_types::member_annotations(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    );
    let captured = captured_containing(&recording, "@template");
    assert_eq!(captured.template_names, vec!["TKey", "TValue"]);
    // The bound lowering (`syntax.rs`'s `declare_into`) was otherwise
    // unpinned by any test: a dropped bound (needed when zipping
    // missing arguments against "the template's bound then mixed") is
    // a real future bug.
    assert_eq!(
        captured.template_bounds,
        vec![
            Some(celerrate_types::TypeId::int(&fixture.db).display(&fixture.db)),
            None,
        ],
    );
}

#[test]
fn named_variables_lower_into_the_variables_field() {
    let fixture = fixture(&["<?php class C { /** @var Collection<User> $items */ public $prop; }"]);
    let recording = register_recording_bridge(&fixture.db);
    let query = member_query(&fixture, "C", MemberKind::Property, "prop");
    let _ = celerrate_types::member_annotations(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    );
    let captured = captured_containing(&recording, "@var");
    assert_eq!(captured.variable_names, vec!["items"]);
}

#[test]
fn a_named_var_on_a_property_still_types_the_property() {
    // Regression: the named `@var Type $name` form is a standard
    // property idiom (both PHPStan and Psalm accept it). Tag extraction
    // cannot know whether the docblock sits above a property or above
    // a statement, so the named form must fill BOTH `variable_values`
    // (for the later inline-narrowing consumer) AND `value_type` (for
    // `declared.rs`, which reads `value_type` for `MemberKind::Property`).
    // Before the fix, the named form only filled `variable_values`, so this
    // untyped-native property silently fell back to `mixed`.
    let fixture = fixture(&["<?php class C { /** @var string $name */ private $name; }"]);
    register_bridge(&fixture.db);
    let query = member_query(&fixture, "C", MemberKind::Property, "name");
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
        celerrate_types::TypeId::string(&fixture.db),
        "a named @var must still type the property, not fall back to mixed",
    );
}
