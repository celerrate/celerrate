//! The per-member lookup: the firewall between the linearized tables
//! and their consumers, the same pattern as `lookup_symbol`. A member
//! added anywhere re-runs the affected linearization, but a lookup
//! whose answer did not change backdates, and the files that asked it
//! are spared.

use std::collections::{HashSet, VecDeque};

use celerrate_db::AnalyzedFileSet;
use celerrate_project::{PhpVersionRange, ProjectConfiguration};
use celerrate_stubs::{StubIndexInput, StubMember, StubMemberKind};

use crate::index::{StubSignatureTable, stub_signature_table};
use crate::linearize::{ClassQuery, MemberOrigin, folded_member_key, linearized_class};
use crate::members::{Member, MemberKind};
use crate::symbols::{SymbolSpace, folded_symbol_key};
use crate::virtual_symbols::{VirtualMember, VirtualMemberKind};

/// One member to look up: its class (a **pre-folded** ClassLike key),
/// its kind, and its **pre-folded** member key (fold with
/// `folded_member_key`, using the queried kind, before interning — so
/// spelling and case variants of one member share one memo).
#[salsa::interned(debug)]
pub struct MemberQuery<'db> {
    #[returns(ref)]
    pub class_key: String,
    pub kind: MemberKind,
    #[returns(ref)]
    pub member_key: String,
}

/// Where one queried member resolved: a source member (its payload, the
/// folded key of its declaring class, and its origin relative to the
/// queried class) or a stub member (its blob payload and the folded key
/// of its declaring stub class).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberResolution {
    Source {
        member: Member,
        owner: String,
        origin: MemberOrigin,
    },
    Stub {
        member: StubMember,
        owner: String,
    },
    /// An annotation-declared member (`@property`, `@method`). Real
    /// members — source and stub alike — shadow virtual members; the
    /// type expressions inside `member` are unresolved text that
    /// `celerrate_types` resolves through the type-syntax registry.
    Virtual {
        member: VirtualMember,
        owner: String,
    },
}

/// Resolves one (class, kind, member) query. The source linearized table
/// wins first; failing that, the stub graph behind each stub edge is
/// walked in edge walk order; and when the queried class is not a source
/// class-like at all, the stub graph is walked from the class key
/// itself. Failing both, a virtual member of the same key — method or
/// property queries only — answers last. `None` when nothing answers.
/// The linearized table's first entry per `(kind, key)` is the
/// precedence winner, so a source member shadows a stub member of the
/// same key, and either shadows a virtual member of the same key.
#[salsa::tracked]
pub fn lookup_member<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> Option<MemberResolution> {
    let class = ClassQuery::new(db, query.class_key(db).clone());
    let kind = query.kind(db);
    let key = query.member_key(db);
    let range = configuration.php_version_range(db);
    let table = stub_signature_table(db, stubs);
    // Kept alive to the end: the virtual-member scan (last resort) reads
    // the same linearized table the source scan above already fetched.
    let linearized = linearized_class(db, files, stubs, configuration, class).as_ref();
    match linearized {
        Some(linearized) => {
            if let Some(entry) = linearized
                .members
                .iter()
                .find(|entry| entry.member.kind == kind && entry.key == *key)
            {
                return Some(MemberResolution::Source {
                    member: entry.member.clone(),
                    owner: entry.owner.clone(),
                    origin: entry.origin.clone(),
                });
            }
            // Fall through to the stub graph behind each stub edge, in
            // walk order.
            for edge in &linearized.ancestry {
                if let Some(stub_key) = &edge.stub
                    && let Some(found) = stub_member(table, range, stub_key, kind, key)
                {
                    return Some(found);
                }
            }
        }
        // Not a source class-like: the stub graph from the key itself.
        None => {
            if let Some(found) = stub_member(table, range, query.class_key(db), kind, key) {
                return Some(found);
            }
        }
    }

    // Virtual members answer last: a real member of the same key,
    // source or stub, always wins (decision 4 of the plan header).
    let virtual_kind = match kind {
        MemberKind::Method => Some(VirtualMemberKind::Method),
        MemberKind::Property => Some(VirtualMemberKind::Property),
        MemberKind::ClassConstant | MemberKind::EnumCase => None,
    };
    if let (Some(virtual_kind), Some(linearized)) = (virtual_kind, linearized)
        && let Some(entry) = linearized
            .virtual_members
            .iter()
            .find(|entry| entry.member.kind == virtual_kind && entry.key == *key)
    {
        return Some(MemberResolution::Virtual {
            member: entry.member.clone(),
            owner: entry.owner.clone(),
        });
    }
    None
}

/// Breadth-first over the compiled parent links from `start`: the first
/// stub member of the queried kind and folded key that exists in the
/// version range. The visited set only guards revisits, so the queue's
/// recorded order fixes the answer.
fn stub_member(
    table: &StubSignatureTable,
    range: PhpVersionRange,
    start: &str,
    kind: MemberKind,
    key: &str,
) -> Option<MemberResolution> {
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();
    queue.push_back(start.to_owned());
    while let Some(class_key) = queue.pop_front() {
        if !visited.insert(class_key.clone()) {
            continue;
        }
        let Some(surface) = table.class(&class_key) else {
            continue;
        };
        for member in &surface.members {
            if member_kind_of(member.kind) == kind
                && folded_member_key(kind, &member.name) == key
                && member.availability.exists_in(range)
            {
                return Some(MemberResolution::Stub {
                    member: member.clone(),
                    owner: class_key,
                });
            }
        }
        for parent in &surface.parents {
            queue.push_back(folded_symbol_key(SymbolSpace::ClassLike, parent));
        }
    }
    None
}

/// The member-lookup kind of one stub member kind: the two spaces line
/// up one to one.
const fn member_kind_of(kind: StubMemberKind) -> MemberKind {
    match kind {
        StubMemberKind::Method => MemberKind::Method,
        StubMemberKind::Property => MemberKind::Property,
        StubMemberKind::ClassConstant => MemberKind::ClassConstant,
        StubMemberKind::EnumCase => MemberKind::EnumCase,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubClassSurface, StubIndex, StubIndexInput, StubMember, StubMemberKind,
        StubSignature, StubSymbol, StubSymbolKind, StubVisibility, VersionedTypeText,
    };

    use super::{MemberQuery, MemberResolution, lookup_member};
    use crate::linearize::{MemberOrigin, folded_member_key};
    use crate::members::MemberKind;
    use crate::plugin::PluginIdentity;
    use crate::symbols::{SymbolSpace, folded_symbol_key};
    use crate::virtual_symbols::{
        VirtualMember, VirtualMemberKind, VirtualSymbolProvider, VirtualSymbolRegistration,
        VirtualSymbolRegistry,
    };

    /// A provider that answers its fixed member set only when the
    /// docblock text carries `@fake`. Duplicated from `linearize.rs`'s
    /// test module (itself duplicated from `virtual_symbols.rs`) — the
    /// crate has no shared test-support module yet, which is already
    /// recorded debt.
    #[derive(Debug)]
    struct FakeProvider {
        members: Vec<VirtualMember>,
    }

    impl VirtualSymbolProvider for FakeProvider {
        fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
            if class_docblock.contains("@fake") {
                self.members.clone()
            } else {
                Vec::new()
            }
        }
    }

    fn identity(name: &str) -> PluginIdentity {
        PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    fn register_fake_provider(fixture: &Fixture, members: Vec<VirtualMember>) {
        let _ = VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
            identity: identity("fake"),
            provider: std::sync::Arc::new(FakeProvider { members }),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    fn virtual_property(name: &str) -> VirtualMember {
        VirtualMember {
            kind: VirtualMemberKind::Property,
            name: name.to_owned(),
            is_static: false,
            type_text: Some("string".to_owned()),
            parameters: Vec::new(),
        }
    }

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
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![
            StubSymbol {
                name: "Exception".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
        ]))
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

    /// A fixture whose stub payload carries real class surfaces. Every
    /// surface key and every parent it names becomes a `StubSymbol`, so
    /// `resolve_ancestor` classifies a source edge as a stub edge.
    fn fixture_with_stub_classes(
        sources: &[&str],
        classes: Vec<(String, StubClassSurface)>,
    ) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let mut names: Vec<String> = Vec::new();
        for (name, surface) in &classes {
            names.push(name.clone());
            for parent in &surface.parents {
                names.push(parent.clone());
            }
        }
        names.sort();
        names.dedup();
        let symbols: Vec<StubSymbol> = names
            .into_iter()
            .map(|name| StubSymbol {
                name,
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            })
            .collect();
        let stubs = StubIndexInput::builder(StubIndex::new(symbols, vec![], classes))
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

    /// A `getMessage(): string` public instance method surface member.
    fn get_message_member() -> StubMember {
        StubMember {
            kind: StubMemberKind::Method,
            name: "getMessage".to_owned(),
            visibility: StubVisibility::Public,
            is_static: false,
            availability: StubAvailability::ALWAYS,
            signature: Some(StubSignature::default()),
            type_text: VersionedTypeText::default(),
            value_text: None,
        }
    }

    fn lookup(
        fixture: &Fixture,
        class_written: &str,
        kind: MemberKind,
        member_written: &str,
    ) -> Option<MemberResolution> {
        let query = MemberQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::ClassLike, class_written),
            kind,
            folded_member_key(kind, member_written),
        );
        lookup_member(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
    }

    #[test]
    fn a_member_resolves_through_the_linearized_table() {
        let fixture = fixture(&[
            "<?php class Base { public function hello() {} }",
            "<?php class Child extends Base {}",
        ]);
        let MemberResolution::Source { owner, origin, .. } =
            lookup(&fixture, "Child", MemberKind::Method, "HELLO").unwrap()
        else {
            panic!("expected a source member");
        };
        assert_eq!(owner, "base");
        assert_eq!(origin, MemberOrigin::Inherited);
    }

    #[test]
    fn a_source_class_inherits_stub_members_through_the_blob() {
        let fixture = fixture_with_stub_classes(
            &["<?php class MyError extends RuntimeException {}"],
            vec![
                (
                    "RuntimeException".to_owned(),
                    StubClassSurface {
                        parents: vec!["Exception".to_owned()],
                        members: vec![],
                    },
                ),
                (
                    "Exception".to_owned(),
                    StubClassSurface {
                        parents: vec![],
                        members: vec![get_message_member()],
                    },
                ),
            ],
        );
        let resolution = lookup(&fixture, "MyError", MemberKind::Method, "GETMESSAGE").unwrap();
        let MemberResolution::Stub { member, owner } = resolution else {
            panic!("expected a stub member");
        };
        assert_eq!(member.name, "getMessage");
        assert_eq!(owner, "exception");
    }

    #[test]
    fn a_stub_only_class_answers_its_own_members() {
        let fixture = fixture_with_stub_classes(
            &["<?php"],
            vec![(
                "Exception".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![get_message_member()],
                },
            )],
        );
        assert!(lookup(&fixture, "Exception", MemberKind::Method, "getmessage").is_some());
        assert!(lookup(&fixture, "Exception", MemberKind::Method, "ghost").is_none());
    }

    #[test]
    fn source_members_shadow_stub_members() {
        let fixture = fixture_with_stub_classes(
            &["<?php class MyError extends Exception { public function getMessage(): string {} }"],
            vec![(
                "Exception".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![get_message_member()],
                },
            )],
        );
        assert!(matches!(
            lookup(&fixture, "MyError", MemberKind::Method, "getmessage"),
            Some(MemberResolution::Source { .. }),
        ));
    }

    #[test]
    fn a_member_outside_its_availability_window_is_absent() {
        // `getMessage` introduced in 8.6, but the range is 8.1-8.5.
        let mut member = get_message_member();
        member.availability = StubAvailability {
            introduced: Some(PhpVersion::new(8, 6)),
            ..StubAvailability::ALWAYS
        };
        let fixture = fixture_with_stub_classes(
            &["<?php"],
            vec![(
                "Exception".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![member],
                },
            )],
        );
        assert!(lookup(&fixture, "Exception", MemberKind::Method, "getmessage").is_none());
    }

    #[test]
    fn an_unknown_member_or_class_answers_none() {
        let fixture = fixture(&["<?php class A { public function f() {} }"]);
        assert!(lookup(&fixture, "A", MemberKind::Method, "missing").is_none());
        assert!(lookup(&fixture, "Ghost", MemberKind::Method, "f").is_none());
        // Kinds are distinct spaces: a method never answers a property.
        assert!(lookup(&fixture, "A", MemberKind::Property, "f").is_none());
    }

    #[test]
    fn a_virtual_member_resolves_when_no_real_member_exists() {
        let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
        register_fake_provider(&fixture, vec![virtual_property("title")]);
        let resolution = lookup(&fixture, "Post", MemberKind::Property, "title");
        match resolution {
            Some(MemberResolution::Virtual { member, owner }) => {
                assert_eq!(member.name, "title");
                assert_eq!(owner, "post");
            }
            other => panic!("expected a virtual resolution, got {other:?}"),
        }
    }

    #[test]
    fn a_real_member_shadows_a_virtual_member_of_the_same_name() {
        let fixture = fixture(&["<?php /** @fake */ class Post { public string $title; }"]);
        register_fake_provider(&fixture, vec![virtual_property("title")]);
        let resolution = lookup(&fixture, "Post", MemberKind::Property, "title");
        assert!(matches!(resolution, Some(MemberResolution::Source { .. })));
    }

    #[test]
    fn virtual_members_answer_only_method_and_property_queries() {
        let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
        register_fake_provider(&fixture, vec![virtual_property("TITLE")]);
        assert!(lookup(&fixture, "Post", MemberKind::ClassConstant, "TITLE").is_none());
    }
}
