//! The overlap oracle (issue #62): every spelling both grammars accept
//! must lower to the identical interned type from both provenances —
//! a stub refinement (the norm path) and a docblock (the bridge path).
//! Deliberate dialect differences are table entries too, so they are
//! documented here rather than invisible: a spelling one side rejects
//! is asserted to be rejected, and a change to either verdict fails
//! this suite.
//!
//! Both provenances share ONE database per entry (`fixture` wires a
//! stub function `f` for the norm path and a class `C::m` for the
//! bridge path into the same `TestDatabase`), so the two `TypeId`s
//! being compared are interned in the same interner — a direct `==`
//! is meaningful.
//!
//! **Verdict encoding.** Neither `declared_function_signature` nor
//! `declared_member_signature` answers a bare `None` when a refined
//! or annotated text fails to lower — both fall back to their native
//! declaration (`Trust::NativeOnly`), which this harness sets to
//! `mixed` on both sides precisely so that fallback is the ONLY way
//! `Trust::NativeOnly` can arise: the subtype judgment's rule 2
//! (`target.is_mixed` holds unconditionally, `judgments.rs`) means
//! any text that DOES lower is trusted as `Refined` regardless of its
//! shape. So `norm_type`/`bridge_type` read `value_trust`, not merely
//! `value_type`: `Trust::NativeOnly` is read back as `None` (the
//! grammar rejected the spelling), anything else as `Some` (the
//! grammar lowered it) — exactly the `Verdict` this table encodes.
//!
//! **Fixture precedents.** The bridge registration (`register_bridge`,
//! `member_query`) mirrors `crates/celerrate_phpdoc_bridge/tests/end_to_end.rs`.
//! The norm (stub-refinement) path mirrors `celerrate_types::declared`'s
//! own `refined_stub_fixture`/`array_keys_refinement` test pattern
//! (`crates/celerrate_types/src/declared.rs`, a free function refined
//! through `celerrate_stubs::StubRefinements`) rebuilt here against
//! `celerrate_types`' public declared-signature query surface, since
//! that helper is a private `#[cfg(test)]` fixture inside
//! `celerrate_types` and cannot be imported from this crate's
//! integration tests.
//!
//! **Expected result (Task 1's TDD red step).** This suite is
//! EXPECTED TO FAIL on exactly the `Status::Active` entry: the
//! confirmed divergence issue #62 exists to catch. The norm lowers it
//! to a real enum-case type; the bridge's `ConstFetch` lowering
//! (`celerrate_phpdoc_bridge/src/lowering.rs`) folds every `Foo::Bar`
//! constant fetch to `mixed`, undecided from a genuine class-constant
//! fetch, so the two sides disagree. A later task repairs the bridge
//! so this entry turns green; until then this file is committed red.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{MemberKind, SymbolSpace, folded_member_key, folded_symbol_key};
use celerrate_source::FileId;
use celerrate_stubs::{
    RefinedSignature, StubAvailability, StubIndex, StubIndexInput, StubRefinements, StubSignature,
    StubSymbol, StubSymbolKind, VersionedTypeText,
};

struct Fixture {
    db: TestDatabase,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

/// One combined database: a genuine PHP source fixture (a `Status`
/// enum, a `User` class, and a `C` class whose untyped method `m`
/// carries `bridge_text` as its `@return` docblock — the bridge path)
/// plus one synthetic stub free function `f` with a native return of
/// `mixed`, refined with `norm_text` as its return-type text (the norm
/// path). Both provenances share this one database and interner.
fn fixture(norm_text: &str, bridge_text: &str) -> Fixture {
    let db = TestDatabase::default();
    let source = format!(
        "<?php enum Status {{ case Active; }} class User {{}} class C {{ /** @return {bridge_text} */ public function m() {{}} }}"
    );
    let handles = vec![SourceFile::new(&db, FileId::new(0), source.into_bytes())];
    let files = AnalyzedFileSet::new(&db, handles);

    let symbols = vec![StubSymbol {
        name: "f".to_owned(),
        kind: StubSymbolKind::Function,
        availability: StubAvailability::ALWAYS,
    }];
    let functions = vec![(
        "f".to_owned(),
        StubSignature {
            parameters: vec![],
            return_type: VersionedTypeText::from_text(Some("mixed".to_owned())),
            by_reference: false,
        },
    )];
    let mut index = StubIndex::new(symbols, functions, vec![]);
    index.set_refinements(StubRefinements::new(
        vec![(
            "f".to_owned(),
            RefinedSignature {
                templates: vec![],
                parameters: vec![],
                return_type: Some(norm_text.to_owned()),
            },
        )],
        vec![],
    ));
    let stubs = StubIndexInput::builder(index)
        .durability(salsa::Durability::HIGH)
        .new(&db);

    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);

    register_bridge(&db);

    Fixture {
        db,
        files,
        stubs,
        configuration,
    }
}

/// Registers the real bridge as the `TypeSyntax` implementation and
/// the virtual-symbol provider, mirroring `end_to_end.rs`'s
/// `register_bridge`.
fn register_bridge(db: &TestDatabase) {
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

/// A folded `MemberQuery` for one class-and-member pair, mirroring
/// `end_to_end.rs`'s own helper.
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

/// The norm path's answer for the fixture's `norm_text`. `None` only
/// when the text fails to lower (`Trust::NativeOnly`, the stub
/// function's fallback to its `mixed` native declaration — the only
/// way that trust can arise here, since a refinement text is always
/// supplied and `mixed` accepts every lowered candidate
/// unconditionally, `judgments.rs` rule 2).
fn norm_type<'db>(fixture: &'db Fixture) -> Option<celerrate_types::TypeId<'db>> {
    let signature = celerrate_types::declared_function_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        celerrate_types::FunctionQuery::new(&fixture.db, "f".to_owned()),
    )
    .unwrap_or_else(|| panic!("the stub function `f` must resolve; check the fixture wiring"));
    match signature.value_trust {
        celerrate_types::Trust::NativeOnly => None,
        _ => Some(signature.value_type),
    }
}

/// The bridge path's answer for the fixture's `bridge_text`, by the
/// same rule: `None` only when the docblock's `@return` text fails to
/// parse at all (`Trust::NativeOnly`, the untyped method's fallback to
/// `mixed`).
fn bridge_type<'db>(fixture: &'db Fixture) -> Option<celerrate_types::TypeId<'db>> {
    let query = member_query(fixture, "C", MemberKind::Method, "m");
    let signature = celerrate_types::declared_member_signature(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        query,
    )
    .unwrap_or_else(|| panic!("member `C::m` must resolve; check the fixture wiring"));
    match signature.value_trust {
        celerrate_types::Trust::NativeOnly => None,
        _ => Some(signature.value_type),
    }
}

/// One table entry: the spelling, and what each provenance must do
/// with it.
struct Entry {
    spelling: &'static str,
    norm: Verdict,
    bridge: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Lowers; when both sides lower, their `TypeId`s must be equal.
    Lowers,
    /// The grammar rejects the spelling (documented dialect gap).
    Rejects,
}

use Verdict::{Lowers, Rejects};

const fn entry(spelling: &'static str, norm: Verdict, bridge: Verdict) -> Entry {
    Entry {
        spelling,
        norm,
        bridge,
    }
}

const TABLE: &[Entry] = &[
    // Keyword atoms and literals: full agreement expected.
    entry("int", Lowers, Lowers),
    entry("string", Lowers, Lowers),
    entry("bool", Lowers, Lowers),
    entry("mixed", Lowers, Lowers),
    entry("array-key", Lowers, Lowers),
    entry("'active'", Lowers, Lowers),
    entry("42", Lowers, Lowers),
    // Composition.
    entry("int|string", Lowers, Lowers),
    entry("Countable&Traversable", Lowers, Lowers),
    entry("?int", Lowers, Lowers),
    entry("?User|string", Lowers, Lowers),
    // Generics and their sugars.
    entry("array<int, string>", Lowers, Lowers),
    entry("array<string>", Lowers, Lowers),
    entry("list<int>", Lowers, Lowers),
    entry("non-empty-list<int>", Lowers, Lowers),
    entry("iterable<int>", Lowers, Lowers),
    entry("iterable<string, int>", Lowers, Lowers),
    // Shapes.
    entry("{id: int, name?: string}", Lowers, Rejects), // norm spelling
    entry("array{id: int, name?: string}", Rejects, Lowers), // dialect spelling
    // Callables.
    entry("callable(int, string=): void", Lowers, Lowers),
    entry("callable(int...): void", Lowers, Lowers),
    // Projections and class-string.
    entry("class-string", Lowers, Lowers),
    entry("class-string<User>", Lowers, Lowers),
    entry("key-of<array<int, string>>", Lowers, Lowers),
    entry("value-of<array<int, string>>", Lowers, Lowers),
    // Enum cases: the confirmed divergence this suite exists to catch.
    entry("Status::Active", Lowers, Lowers),
    // Documented dialect gaps (ranges spell differently by design).
    entry("int<1..5>", Lowers, Rejects),
    entry("int<1, 5>", Rejects, Lowers),
    entry("User[]", Rejects, Lowers),
];

#[test]
fn every_table_entry_agrees_or_documents_its_dialect_gap() {
    for candidate in TABLE {
        let fixture = fixture(candidate.spelling, candidate.spelling);
        let norm_value = norm_type(&fixture);
        let bridge_value = bridge_type(&fixture);
        match (candidate.norm, candidate.bridge) {
            (Verdict::Lowers, Verdict::Lowers) => {
                let norm_value = norm_value.unwrap_or_else(|| {
                    panic!("norm path must lower {:?}, got None", candidate.spelling)
                });
                let bridge_value = bridge_value.unwrap_or_else(|| {
                    panic!("bridge path must lower {:?}, got None", candidate.spelling)
                });
                assert_eq!(
                    norm_value, bridge_value,
                    "provenance mismatch for {:?}: norm and bridge must intern to the same TypeId",
                    candidate.spelling,
                );
            }
            (Verdict::Lowers, Verdict::Rejects) => {
                assert!(
                    norm_value.is_some(),
                    "norm path must lower {:?}",
                    candidate.spelling,
                );
                assert!(
                    bridge_value.is_none(),
                    "bridge path must reject {:?} (documented dialect gap)",
                    candidate.spelling,
                );
            }
            (Verdict::Rejects, Verdict::Lowers) => {
                assert!(
                    norm_value.is_none(),
                    "norm path must reject {:?} (documented dialect gap)",
                    candidate.spelling,
                );
                assert!(
                    bridge_value.is_some(),
                    "bridge path must lower {:?}",
                    candidate.spelling,
                );
            }
            (Verdict::Rejects, Verdict::Rejects) => {
                assert!(
                    norm_value.is_none(),
                    "norm path must reject {:?}",
                    candidate.spelling,
                );
                assert!(
                    bridge_value.is_none(),
                    "bridge path must reject {:?}",
                    candidate.spelling,
                );
            }
        }
    }
}

#[test]
fn the_norm_bare_shape_and_the_dialect_array_prefixed_shape_lower_to_the_same_type() {
    // Shape-spelling caveat: the norm writes shapes bare (`{...}`); the
    // dialect prefixes them (`array{...}`). Same fixture (one shared
    // interner): the two spellings must intern to the identical
    // `TypeId` — equivalence across different spellings is the point
    // of documented sugar.
    let fixture = fixture("{id: int}", "array{id: int}");
    let norm_value =
        norm_type(&fixture).unwrap_or_else(|| panic!("the norm's bare shape must lower"));
    let bridge_value = bridge_type(&fixture)
        .unwrap_or_else(|| panic!("the dialect's array-prefixed shape must lower"));
    assert_eq!(
        norm_value, bridge_value,
        "the norm's bare `{{id: int}}` and the dialect's `array{{id: int}}` must be the same type",
    );
}
