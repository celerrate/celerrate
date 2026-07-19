//! Shared test-only fixture for the checks family: the same
//! `Fixture`/`fixture`/`handle_of` idiom task 2's `mod.rs` test module
//! built, lifted here so `receivers.rs` (and the task 4-6 walkers)
//! share one builder rather than each re-deriving it — the crate's
//! existing `test_support`/`minimal_stub_index` pattern
//! (`inheritance/test_support.rs`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    AstId, BodyQuery, PluginIdentity, UseTables, VirtualMember, VirtualMemberKind,
    VirtualSymbolProvider, VirtualSymbolRegistration, VirtualSymbolRegistry, body_ir, item_tree,
};
use celerrate_source::FileId;
use celerrate_stubs::{
    StubAvailability, StubClassSurface, StubIndex, StubIndexInput, StubMember, StubMemberKind,
    StubParameter, StubSignature, StubSymbol, StubSymbolKind, StubVisibility, VersionedTypeText,
};

use super::{CheckContext, TypedVerdictKind, typed_file_verdicts};
use crate::inference::{BodyOwner, body_owner, inferred_body_types};

pub(crate) struct Fixture {
    pub(crate) db: TestDatabase,
    pub(crate) handles: Vec<SourceFile>,
    pub(crate) files: AnalyzedFileSet,
    pub(crate) stubs: StubIndexInput,
    pub(crate) configuration: ProjectConfiguration,
}

/// The default fixture: the crate's minimal realistic stub surface
/// (`inheritance::test_support::minimal_stub_index`), the same default
/// every other module's fixture carries.
pub(crate) fn fixture(sources: &[&str]) -> Fixture {
    fixture_with_stubs(
        sources,
        crate::inheritance::test_support::minimal_stub_index(),
    )
}

/// A fixture over a caller-supplied stub index, for the synthetic stub
/// surfaces a test needs (decision 7's `UnitEnum`/`BackedEnum`, a
/// single named class) that the default minimal index does not carry.
pub(crate) fn fixture_with_stubs(sources: &[&str], stub_index: StubIndex) -> Fixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(stub_index)
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
        handles,
        files,
        stubs,
        configuration,
    }
}

pub(crate) fn handle_of(fixture: &Fixture, index: usize) -> SourceFile {
    fixture.handles[index]
}

/// A fixture whose stub payload carries a single named class with no
/// parents or members (`StubIndex::from_symbols`) — just enough to
/// exercise `class_surface`'s `Stub` branch for that one class, and (for
/// `stdClass` specifically) the dynamic-property stub-ancestor rule.
/// Built on the `fixture_with_stub_classes`/`fixture_with_stub_class`
/// idiom already established in `celerrate_semantics::member_lookup`'s
/// test module.
pub(crate) fn fixture_with_stub_class(sources: &[&str], name: &str) -> Fixture {
    fixture_with_stubs(
        sources,
        StubIndex::from_symbols(vec![StubSymbol {
            name: name.to_owned(),
            kind: StubSymbolKind::Class,
            availability: StubAvailability::ALWAYS,
        }]),
    )
}

/// A fixture whose stub payload declares one enum entirely in the
/// compiled surface — a `StubSymbolKind::Enum` symbol with the given
/// case names as `StubMemberKind::EnumCase` members — and carries no
/// matching source declaration at all (mirroring a PHP built-in enum).
/// For pinning the stub-only-enum classification `is_enum_key` alone
/// gets right: `class_kind` answers `None` for a class-like with no
/// source declaration, so the constant-or-case dual lookup must not
/// consult it directly.
pub(crate) fn fixture_with_stub_enum(sources: &[&str], name: &str, cases: &[&str]) -> Fixture {
    let members = cases
        .iter()
        .map(|case| StubMember {
            kind: StubMemberKind::EnumCase,
            name: (*case).to_owned(),
            visibility: StubVisibility::Public,
            is_static: false,
            availability: StubAvailability::ALWAYS,
            signature: None,
            type_text: VersionedTypeText::default(),
            value_text: None,
        })
        .collect();
    let index = StubIndex::new(
        vec![StubSymbol {
            name: name.to_owned(),
            kind: StubSymbolKind::Enum,
            availability: StubAvailability::ALWAYS,
        }],
        vec![],
        vec![(
            name.to_owned(),
            StubClassSurface {
                parents: vec![],
                members,
            },
        )],
    );
    fixture_with_stubs(sources, index)
}

/// A synthetic stub index carrying `UnitEnum` (with `cases`) and
/// `BackedEnum` (with `from`, `tryFrom`) — the compiled surface decision
/// 7's implicit enum edges (`linearize.rs`) rely on being present. Shared
/// by `receivers.rs`'s and `members.rs`'s synthetic-stub tests, built on
/// the member-lookup synthetic-stub idiom (`fixture_with_stub_classes`
/// in `member_lookup.rs`).
pub(crate) fn fixture_with_stub_enum_interfaces(sources: &[&str]) -> Fixture {
    fn interface_symbol(name: &str) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind: StubSymbolKind::Interface,
            availability: StubAvailability::ALWAYS,
        }
    }
    fn static_method(name: &str, return_type: &str, parameters: Vec<StubParameter>) -> StubMember {
        StubMember {
            kind: StubMemberKind::Method,
            name: name.to_owned(),
            visibility: StubVisibility::Public,
            is_static: true,
            availability: StubAvailability::ALWAYS,
            signature: Some(StubSignature {
                parameters,
                return_type: VersionedTypeText::from_text(Some(return_type.to_owned())),
                by_reference: false,
            }),
            type_text: VersionedTypeText::default(),
            value_text: None,
        }
    }
    // `from`/`tryFrom` carry a single `$value` parameter, matching the
    // real `BackedEnum` surface (`from(int|string $value): static`) —
    // task 9's arity check now runs over every resolved call, so a
    // zero-parameter synthetic signature here would misreport a
    // genuine one-argument call as excess.
    fn value_parameter() -> StubParameter {
        StubParameter {
            name: "value".to_owned(),
            type_text: VersionedTypeText::from_text(Some("int|string".to_owned())),
            optional: false,
            by_reference: false,
            variadic: false,
            availability: StubAvailability::ALWAYS,
        }
    }
    let index = StubIndex::new(
        vec![interface_symbol("UnitEnum"), interface_symbol("BackedEnum")],
        vec![],
        vec![
            (
                "UnitEnum".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![static_method("cases", "static[]", vec![])],
                },
            ),
            (
                "BackedEnum".to_owned(),
                StubClassSurface {
                    parents: vec!["UnitEnum".to_owned()],
                    members: vec![
                        static_method("from", "static", vec![value_parameter()]),
                        static_method("tryFrom", "?static", vec![value_parameter()]),
                    ],
                },
            ),
        ],
    );
    fixture_with_stubs(sources, index)
}

/// A `CheckContext` for the declaration numbered `body_index` in file
/// 0, built the same way `body_typed_verdicts` builds one. Every test
/// fixture that calls this is built to have a body there, so a missing
/// IR or inferred body is an unconditional test-only panic, not a
/// production path.
pub(crate) fn context_for(fixture: &Fixture, body_index: u32) -> CheckContext<'_, '_> {
    let file = handle_of(fixture, 0);
    let db = &fixture.db;
    let body = BodyQuery::new(
        db,
        AstId {
            file: FileId::new(0),
            index: body_index,
        },
    );
    let ir = body_ir(db, file, body)
        .as_ref()
        .expect("the fixture body has an IR");
    let inferred = inferred_body_types(
        db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        file,
        body,
    )
    .as_ref()
    .expect("the fixture body types");
    let owner = body_owner(db, file, body).as_ref();
    let namespace = match owner {
        Some(BodyOwner::Function(function)) => function.namespace.clone(),
        Some(BodyOwner::Method { namespace, .. }) => namespace.clone(),
        None => String::new(),
    };
    CheckContext {
        db,
        files: fixture.files,
        stubs: fixture.stubs,
        configuration: fixture.configuration,
        file,
        body: body.ast_id(db),
        ir,
        inferred,
        owner,
        tables: UseTables::for_namespace(item_tree(db, file), &namespace),
        namespace,
        dependencies: std::cell::RefCell::new(std::collections::BTreeSet::new()),
    }
}

/// A provider that recognizes the literal PHPDoc `@method` and
/// `@property` tag conventions (`@method <ReturnType> <name>(...)`,
/// `@property <Type> $<name>`) — just enough to exercise the
/// virtual-member integration `members.rs`'s guillotine leans on
/// (design section 8: `@method`/`@property` "count as existing"). The
/// real dialect lives in `celerrate_phpdoc_bridge`, a crate above this
/// one in the dependency DAG that cannot be depended on from here; a
/// duplicated minimal parser is the codebase's own precedent for this
/// gap (`celerrate_semantics`'s own test modules carry an equivalent
/// `FakeProvider`, keyed on a bare `@fake` marker since none of them
/// exercise the real tag text).
#[derive(Debug)]
struct DocblockMemberProvider;

impl VirtualSymbolProvider for DocblockMemberProvider {
    fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
        let methods = class_docblock.split("@method").skip(1).filter_map(|rest| {
            let name_token = rest.split_whitespace().find(|token| token.contains('('))?;
            let name = name_token.split('(').next()?;
            (!name.is_empty())
                .then(|| VirtualMember::new(VirtualMemberKind::Method, name.to_owned()))
        });
        let properties = class_docblock
            .split("@property")
            .skip(1)
            .filter_map(|rest| {
                let name_token = rest
                    .split_whitespace()
                    .find(|token| token.starts_with('$'))?;
                let name = name_token.trim_start_matches('$');
                (!name.is_empty())
                    .then(|| VirtualMember::new(VirtualMemberKind::Property, name.to_owned()))
            });
        methods.chain(properties).collect()
    }
}

/// The shared checks-family test entry point: one file's `typed_file_verdicts`
/// kinds, over the default fixture with a `DocblockMemberProvider`
/// registered (design section 8's virtual-member surface). Every
/// `checks` module's test suite shares this rather than re-deriving the
/// `typed_file_verdicts` plumbing — `members.rs`'s tests first built it
/// (as `method_verdicts`), promoted here once `nullability.rs`'s tests
/// needed the same fixture-to-verdicts path (task 6).
pub(crate) fn family_verdicts(source: &str) -> Vec<TypedVerdictKind> {
    let fixture = fixture(&[source]);
    let _ = VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
        identity: PluginIdentity {
            name: "test-docblock-member".to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        },
        provider: std::sync::Arc::new(DocblockMemberProvider),
    }])
    .durability(salsa::Durability::HIGH)
    .new(&fixture.db);
    typed_file_verdicts(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        handle_of(&fixture, 0),
    )
    .verdicts
    .iter()
    .map(|verdict| verdict.kind.clone())
    .collect()
}
