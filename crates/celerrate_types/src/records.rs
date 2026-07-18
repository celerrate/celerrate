//! The class-surface and function-signature digests: one blake3 digest
//! over everything a judgment could have consulted through
//! `lookup_member`, `member_existence`, an ancestry walk, or declared-
//! signature resolution, so a single digest compare revalidates a
//! cached signature (tasks 7-9). The digest is over RESOLVED signatures
//! (through [`crate::declared::declared_member_signature`] and
//! [`crate::declared::declared_function_signature`]), so an annotation-
//! layer edit flips it exactly as a native-signature edit does; a body
//! edit never does, because member boundaries are signature-granular
//! ([`celerrate_semantics::member_tree`]'s own invalidation boundary).
//!
//! Virtual members (`@method`/`@property`) participate with their FULL
//! resolved payload, never existence alone: a type edit on an existing
//! virtual member must flip the digest just as a native signature edit
//! does.
//!
//! Canonical ordering is structural throughout: the linearized table's
//! own deterministic order (`LinearizedClass::members` and
//! `::virtual_members` are already sorted, first entry per `(kind,
//! key)` wins), never derived from an interner handle — handle order is
//! timing- and process-dependent, which is exactly what
//! `the_digest_is_stable_across_identical_projects` pins against.
//!
//! `TypeId` never enters the projection directly: every type is
//! mirrored through [`crate::stored::StoredType`] first (via
//! [`crate::stored::StoredSignature::of`]), so the digest is a pure
//! function of structural facts, not of process-local interner state.
//!
//! **Recorded scope boundary (plan 9a decision 3).** The class-like's
//! own `DeclarationKind` participates (via
//! [`celerrate_semantics::class_declaration_kind`]), but class-level
//! `abstract`/`final`/`readonly` deliberately do not — see
//! [`class_surface_digest`]'s own rustdoc for the fact and the standing
//! obligation this leaves behind.

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    ClassQuery, DeclarationKind, MemberKind, MemberQuery, VirtualMemberKind, Visibility,
    class_declaration_kind, linearized_class,
};
use celerrate_stubs::StubIndexInput;
use serde::Serialize;

use crate::declared::{FunctionQuery, declared_function_signature, declared_member_signature};
use crate::stored::{StoredSignature, digest_of};

/// The canonical projection of one class's whole lookup surface:
/// everything `lookup_member`, `member_existence`, a judgment's
/// ancestry walk, or declared-signature resolution could consult. One
/// digest compare revalidates all of it.
///
/// Does NOT carry class-level `abstract`/`final`/`readonly` — see
/// [`class_surface_digest`]'s rustdoc for why (plan 9a decision 3's
/// recorded scope boundary).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SurfaceProjection {
    /// The class-like's own `DeclarationKind` discriminant (stable,
    /// hand-assigned order — never `as u8` on the upstream enum, so a
    /// future upstream variant reorder cannot silently reshuffle this).
    kind: u8,
    members: Vec<SurfaceMember>,
    virtual_members: Vec<SurfaceVirtualMember>,
    /// Ancestor folded keys, nearest-first walk order — the exact list
    /// `declared_member_signature`'s own inheritance walk consults.
    ancestry: Vec<String>,
    stub_ancestors: Vec<String>,
    cyclic: bool,
    has_opaque_edge: bool,
    /// The five `MagicMarkers` fields, in their declared field order:
    /// `has_magic_get`, `has_magic_set`, `has_magic_call`,
    /// `has_magic_call_static`, `allows_dynamic_properties`.
    magic: (bool, bool, bool, bool, bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SurfaceMember {
    kind: u8,
    key: String,
    owner: String,
    is_static: bool,
    visibility: u8,
    signature: Option<StoredSignature>,
}

/// One annotation-declared member, payload included: a `@method` or
/// `@property` type edit must flip the digest, never existence alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SurfaceVirtualMember {
    kind: u8,
    key: String,
    owner: String,
    signature: Option<StoredSignature>,
}

/// Stable, hand-assigned discriminants — deliberately not `as u8` on
/// the upstream enums, so an upstream variant reorder cannot silently
/// change every persisted digest's meaning without this file changing
/// too.
fn member_kind_rank(kind: MemberKind) -> u8 {
    match kind {
        MemberKind::Method => 0,
        MemberKind::Property => 1,
        MemberKind::ClassConstant => 2,
        MemberKind::EnumCase => 3,
    }
}

fn virtual_member_kind_rank(kind: VirtualMemberKind) -> u8 {
    match kind {
        VirtualMemberKind::Method => 0,
        VirtualMemberKind::Property => 1,
    }
}

fn visibility_rank(visibility: Visibility) -> u8 {
    match visibility {
        Visibility::Public => 0,
        Visibility::Protected => 1,
        Visibility::Private => 2,
    }
}

fn declaration_kind_rank(kind: DeclarationKind) -> u8 {
    match kind {
        DeclarationKind::Class => 0,
        DeclarationKind::Interface => 1,
        DeclarationKind::Trait => 2,
        DeclarationKind::Enum => 3,
        DeclarationKind::Function => 4,
        DeclarationKind::Constant => 5,
    }
}

/// One blake3 digest over a class's whole lookup surface: `None` when
/// `class` does not name a source class-like (a stub, or an unknown
/// key) — mirrors `linearized_class`'s own `None` exactly, since the
/// projection has nothing to build from.
///
/// **Recorded scope boundary (plan 9a decision 3).** Decision 3
/// names class-level `abstract`/`final`/`readonly` among the facts
/// this digest covers, alongside `DeclarationKind`. Only
/// `DeclarationKind` is implemented: `celerrate_semantics` exposes no
/// class-level abstract/final/readonly fact at all today —
/// `MemberFlags::{is_abstract,is_final,is_readonly}` exists, but only
/// per MEMBER (`Member.flags`); `ClassMembers` and `Declaration` carry
/// `kind: DeclarationKind` and nothing else at the class level. There is
/// therefore no fact for this digest to read, and — checked
/// exhaustively across this crate — no type-engine judgment consults a
/// class-level abstract/final/readonly modifier either, so nothing
/// downstream silently trusts a stale one. This mirrors the standing
/// obligation decision 5 states for [`crate::dynamic_type_provider`]'s
/// cross-file providers: if a future judgment starts reading a
/// class-level modifier, the semantics-layer projection
/// (`ClassMembers`/`Declaration`) and this digest's `SurfaceProjection`
/// must both grow to cover it BEFORE that judgment ships — never after.
#[salsa::tracked]
pub fn class_surface_digest<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> Option<[u8; 32]> {
    let linearized = linearized_class(db, files, stubs, configuration, class).as_ref()?;
    let root_key = class.key(db).clone();
    let kind = class_declaration_kind(db, files, &root_key)?;

    let members = linearized
        .members
        .iter()
        .map(|entry| {
            let query =
                MemberQuery::new(db, root_key.clone(), entry.member.kind, entry.key.clone());
            let signature = declared_member_signature(db, files, stubs, configuration, query)
                .map(|signature| StoredSignature::of(db, &signature));
            SurfaceMember {
                kind: member_kind_rank(entry.member.kind),
                key: entry.key.clone(),
                owner: entry.owner.clone(),
                is_static: entry.member.flags.is_static,
                visibility: visibility_rank(entry.member.flags.visibility),
                signature,
            }
        })
        .collect();

    let virtual_members = linearized
        .virtual_members
        .iter()
        .map(|entry| {
            let member_kind = match entry.member.kind {
                VirtualMemberKind::Method => MemberKind::Method,
                VirtualMemberKind::Property => MemberKind::Property,
            };
            let query = MemberQuery::new(db, root_key.clone(), member_kind, entry.key.clone());
            let signature = declared_member_signature(db, files, stubs, configuration, query)
                .map(|signature| StoredSignature::of(db, &signature));
            SurfaceVirtualMember {
                kind: virtual_member_kind_rank(entry.member.kind),
                key: entry.key.clone(),
                owner: entry.owner.clone(),
                signature,
            }
        })
        .collect();

    let ancestry = crate::declared::ancestors_in_walk_order(&root_key, linearized);

    let projection = SurfaceProjection {
        kind: declaration_kind_rank(kind),
        members,
        virtual_members,
        ancestry,
        stub_ancestors: linearized.stub_ancestors.clone(),
        cyclic: linearized.cyclic,
        has_opaque_edge: linearized.has_opaque_edge,
        magic: (
            linearized.magic.has_magic_get,
            linearized.magic.has_magic_set,
            linearized.magic.has_magic_call,
            linearized.magic.has_magic_call_static,
            linearized.magic.allows_dynamic_properties,
        ),
    };
    digest_of(&projection)
}

/// One blake3 digest over a free function's resolved signature alone:
/// `None` when the signature itself is `None` (an unresolved callee is
/// a recordable answer, compared as `None` at validation).
#[salsa::tracked]
pub fn function_signature_digest<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: FunctionQuery<'db>,
) -> Option<[u8; 32]> {
    let signature = declared_function_signature(db, files, stubs, configuration, query)?;
    digest_of(&StoredSignature::of(db, &signature))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::{ClassQuery, SymbolSpace, folded_symbol_key};
    use celerrate_source::FileId;
    use celerrate_stubs::StubIndexInput;

    use super::{class_surface_digest, function_signature_digest};
    use crate::declared::FunctionQuery;
    use crate::type_syntax::{
        AnnotationSite, ParsedAnnotations, TypeSyntax, TypeSyntaxRegistration, TypeSyntaxRegistry,
    };

    struct Fixture {
        db: TestDatabase,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    }

    /// Copied verbatim from `declared.rs`'s own `fixture` helper (the
    /// brief's instruction: one in-memory project, built the same way,
    /// never a new invented shape).
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
        let stubs = StubIndexInput::builder(crate::inheritance::test_support::minimal_stub_index())
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

    fn class_query<'db>(fixture: &'db Fixture, class_written: &str) -> ClassQuery<'db> {
        ClassQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::ClassLike, class_written),
        )
    }

    fn class_digest(fixture: &Fixture, class_written: &str) -> Option<[u8; 32]> {
        let class = class_query(fixture, class_written);
        class_surface_digest(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            class,
        )
    }

    fn function_digest(fixture: &Fixture, function_written: &str) -> Option<[u8; 32]> {
        let query = FunctionQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::Function, function_written),
        );
        function_signature_digest(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
    }

    /// Registers a `TypeSyntax` fake that parses any docblock
    /// containing `@return` to `return_type: Some(int)`, and parses the
    /// bare expressions `"int"` and `"string"` to `int` and `string`
    /// respectively (refusing anything else) — two DISTINCT, both
    /// validly resolved types, so a test can drive a virtual-member
    /// type edit through two real answers rather than through a
    /// resolved-vs-unparseable fallback. Extended beyond
    /// `declared.rs`'s own `FakeReturnSyntax` (which only recognizes
    /// `"int"`) for exactly that reason; still duplicated from it
    /// otherwise (recorded debt shared with that module: no shared
    /// test-support fake across the crate's test modules).
    fn register_fake_syntax(fixture: &Fixture) {
        let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
            identity: fake_identity("fake-return"),
            implementation: std::sync::Arc::new(FakeReturnSyntax),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    fn fake_identity(name: &str) -> celerrate_semantics::PluginIdentity {
        celerrate_semantics::PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    #[derive(Debug)]
    struct FakeReturnSyntax;

    impl TypeSyntax for FakeReturnSyntax {
        fn can_parse(&self, docblock: &str) -> bool {
            docblock.contains("@return")
        }
        fn parse_docblock<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            _docblock: &str,
        ) -> ParsedAnnotations<'db> {
            ParsedAnnotations {
                return_type: Some(crate::representation::TypeId::int(site.database())),
                ..ParsedAnnotations::default()
            }
        }
        fn parse_type_expression<'db>(
            &self,
            site: &AnnotationSite<'db, '_>,
            expression: &str,
        ) -> Option<crate::representation::TypeId<'db>> {
            match expression {
                "int" => Some(crate::representation::TypeId::int(site.database())),
                "string" => Some(crate::representation::TypeId::string(site.database())),
                _ => None,
            }
        }
    }

    #[test]
    fn the_digest_is_stable_across_identical_projects() {
        let source = "<?php class C { public function f(int $x): string { return \"y\"; } }";
        let first = fixture(&[source]);
        let second = fixture(&[source]);
        let first_digest = class_digest(&first, "C");
        let second_digest = class_digest(&second, "C");
        // Guard first: an equality check between two `None`s would pass
        // vacuously and prove nothing about stability.
        assert!(first_digest.is_some());
        assert!(second_digest.is_some());
        assert_eq!(first_digest, second_digest);
    }

    #[test]
    fn adding_a_member_flips_the_digest() {
        let without = fixture(&["<?php class C { public function f(): int { return 1; } }"]);
        let with = fixture(&[
            "<?php class C { public function f(): int { return 1; } public string $x; }",
        ]);
        assert_ne!(class_digest(&without, "C"), class_digest(&with, "C"));
    }

    #[test]
    fn editing_a_signature_flips_the_digest() {
        let int_return = fixture(&["<?php class C { public function f(): int {} }"]);
        let string_return = fixture(&["<?php class C { public function f(): string {} }"]);
        assert_ne!(
            class_digest(&int_return, "C"),
            class_digest(&string_return, "C")
        );
    }

    #[test]
    fn editing_an_annotation_flips_the_digest() {
        let bare = fixture(&["<?php class C { public function f(): void {} } "]);
        register_fake_syntax(&bare);
        let annotated =
            fixture(&["<?php class C { /** @return */ public function f(): void {} } "]);
        register_fake_syntax(&annotated);
        assert_ne!(class_digest(&bare, "C"), class_digest(&annotated, "C"));
    }

    #[test]
    fn an_ancestry_change_flips_the_digest() {
        let no_extends =
            fixture(&["<?php class A { public function f(): int { return 1; } } class B {}"]);
        let extends = fixture(&[
            "<?php class A { public function f(): int { return 1; } } class B extends A {}",
        ]);
        assert_ne!(class_digest(&no_extends, "B"), class_digest(&extends, "B"));
    }

    #[test]
    fn a_magic_marker_flips_the_digest() {
        let without = fixture(&["<?php class C {}"]);
        let with = fixture(&["<?php class C { public function __get($name) {} }"]);
        assert_ne!(class_digest(&without, "C"), class_digest(&with, "C"));
    }

    #[test]
    fn editing_a_virtual_member_type_flips_the_digest() {
        // Both fixtures' virtual-member type text resolves to a real,
        // distinct type (`int` and `string`, both recognized by
        // `FakeReturnSyntax::parse_type_expression`), so the flip below
        // is driven by "type A vs type B, both validly resolved" — the
        // brief's `@method User find()` -> `@method Order find()`
        // scenario — never by "resolved vs unparseable fallback" (that
        // weaker claim is already covered by
        // `an_unparseable_virtual_type...`-style behavior tested in
        // `declared.rs`, and would overlap with
        // `editing_an_annotation_flips_the_digest` here).
        let as_int = fixture(&["<?php /** @fake @method int find() */ class C {}"]);
        register_virtual_find(&as_int, Some("int".to_owned()));
        register_fake_syntax(&as_int);
        let as_string = fixture(&["<?php /** @fake @method string find() */ class C {}"]);
        register_virtual_find(&as_string, Some("string".to_owned()));
        register_fake_syntax(&as_string);
        let int_digest = class_digest(&as_int, "C");
        let string_digest = class_digest(&as_string, "C");
        // Both sides must have actually resolved (through the Virtual
        // arm's own `Trust::Refined` path), not merely differ because
        // one side silently fell back to `None`/unparseable.
        assert!(int_digest.is_some());
        assert!(string_digest.is_some());
        assert_ne!(int_digest, string_digest);
    }

    fn register_virtual_find(fixture: &Fixture, type_text: Option<String>) {
        use celerrate_semantics::{
            VirtualMember, VirtualMemberKind, VirtualSymbolProvider, VirtualSymbolRegistration,
            VirtualSymbolRegistry,
        };

        #[derive(Debug)]
        struct FakeProvider {
            type_text: Option<String>,
        }

        impl VirtualSymbolProvider for FakeProvider {
            fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
                if class_docblock.contains("@fake") {
                    vec![VirtualMember {
                        kind: VirtualMemberKind::Method,
                        name: "find".to_owned(),
                        is_static: true,
                        type_text: self.type_text.clone(),
                        parameters: Vec::new(),
                    }]
                } else {
                    Vec::new()
                }
            }
        }

        let _ = VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
            identity: fake_identity("fake-virtual"),
            provider: std::sync::Arc::new(FakeProvider { type_text }),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    #[test]
    fn a_declaration_kind_change_flips_the_digest() {
        // `celerrate_semantics` exposes `DeclarationKind` at the class
        // level, but no class-level abstract/final/readonly flag (see
        // this module's doc comment): the pinned scenario here is the
        // one the real surface actually carries.
        let as_class = fixture(&["<?php class C { public function f(): int {} }"]);
        let as_interface = fixture(&["<?php interface C { public function f(): int; }"]);
        assert_ne!(
            class_digest(&as_class, "C"),
            class_digest(&as_interface, "C")
        );
    }

    #[test]
    fn a_body_edit_does_not_flip_the_digest() {
        let returns_one = fixture(&["<?php class C { public function f(): int { return 1; } }"]);
        let returns_two = fixture(&["<?php class C { public function f(): int { return 2; } }"]);
        let first_digest = class_digest(&returns_one, "C");
        let second_digest = class_digest(&returns_two, "C");
        // Guard first: a regression that made the digest answer `None`
        // for both fixtures would otherwise pass this "unchanged"
        // assertion vacuously, while claiming to guard the
        // member-boundary "a body edit must never invalidate" economics.
        assert!(first_digest.is_some());
        assert!(second_digest.is_some());
        assert_eq!(first_digest, second_digest);
    }

    #[test]
    fn a_non_source_key_answers_none() {
        let fixture = fixture(&["<?php class C {}"]);
        // Both keys below hit the SAME early return: `linearized_class`'s
        // internal `fetch()` resolves the root key exclusively through
        // the source symbol table (`lookup_class_declaration`) and never
        // consults the stub index for it, so a compiled-stub key and a
        // wholly unknown key are indistinguishable to this query — there
        // is no "stub vs unknown" code path here, only "not a source
        // class-like". Both are exercised anyway, as two independent
        // spellings of that one case.
        assert_eq!(class_digest(&fixture, "ArrayIterator"), None);
        assert_eq!(class_digest(&fixture, "TotallyUnknownClass"), None);
    }

    #[test]
    fn a_function_signature_digest_flips_on_signature_edits_only() {
        let int_return = fixture(&["<?php function f(): int {} "]);
        let string_return = fixture(&["<?php function f(): string {} "]);
        assert_ne!(
            function_digest(&int_return, "f"),
            function_digest(&string_return, "f")
        );

        let bare = fixture(&["<?php function f(): void {} "]);
        register_fake_syntax(&bare);
        let annotated = fixture(&["<?php /** @return */ function f(): void {} "]);
        register_fake_syntax(&annotated);
        assert_ne!(
            function_digest(&bare, "f"),
            function_digest(&annotated, "f")
        );

        let returns_one = fixture(&["<?php function f(): int { return 1; } "]);
        let returns_two = fixture(&["<?php function f(): int { return 2; } "]);
        let first_digest = function_digest(&returns_one, "f");
        let second_digest = function_digest(&returns_two, "f");
        // Guard first, same reasoning as `a_body_edit_does_not_flip_
        // the_digest`: two `None`s would satisfy the equality check
        // vacuously without proving the body edit was actually spared.
        assert!(first_digest.is_some());
        assert!(second_digest.is_some());
        assert_eq!(first_digest, second_digest);

        let unresolved = fixture(&["<?php "]);
        assert_eq!(function_digest(&unresolved, "totallyUnknownFunction"), None);
    }
}
