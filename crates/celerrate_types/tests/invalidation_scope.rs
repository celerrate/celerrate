//! Invalidation-scope pins for the typed layer: the typed judgments
//! must ride the member boundary's early cutoff, so a method-body edit
//! backdates the member tree and a memoized subtype verdict that
//! consulted the hierarchy does not recompute (the original pin). The
//! remaining five pins cover the declared layer's own edit classes —
//! a method-body edit, a docblock prose edit, a return-type edit, a
//! default-value edit, and an edit to an unrelated member's signature
//! — confirming each either reaches `declared_member_signature` or
//! spares it, and that a changed declared type reruns dependent
//! verdicts while an unchanged one spares them.
//!
//! The final four pins (task 12) cover the design's harness-2 edit
//! classes at the inference layer itself, closing the plan: a callee
//! body edit with an identical inferred return spares the caller, a
//! prose docblock edit re-runs no typed inference, a default-value
//! edit invalidates the signature's dependent body, and editing one
//! member's signature spares a sibling member's body inference.

#![allow(clippy::unwrap_used)]
#![allow(clippy::indexing_slicing)]

use std::sync::Arc;

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    AstId, BodyQuery, MemberKind, MemberQuery, PluginIdentity, SymbolSpace, folded_member_key,
    folded_symbol_key,
};
use celerrate_source::FileId;
use celerrate_stdlib_provider::StdlibProvider;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{
    AnnotationSite, DynamicTypeProviderRegistration, DynamicTypeProviderRegistry, FunctionQuery,
    InferenceContext, MethodQuery, ParsedAncestor, ParsedAnnotations, ParsedTemplate, Proof,
    TypeId, TypeSyntax, TypeSyntaxRegistration, TypeSyntaxRegistry, declared_member_signature,
    inferred_body_types, inferred_function_return, inferred_method_return, subtype_of,
};
use salsa::Setter;

/// Counts how many times a query appears in an executed-query log (the
/// `celerrate_semantics` invalidation-scope tests' `executions_of`
/// pattern, duplicated here: no shared test-support module exists per
/// the design).
fn executions_of(log: &[String], query: &str) -> usize {
    let prefix = format!("{query}(");
    log.iter()
        .filter(|entry| entry.contains(prefix.as_str()))
        .count()
}

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
    // Deliberately empty (issue #36's fixed decision 3): this suite pins
    // salsa execution counts, where a stub surface adds resolution
    // noise without observing stub behaviour, and a separate
    // compilation unit cannot reach `pub(crate)` test support anyway.
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

/// One source file plus the whole-project inputs: an empty stub index
/// kept at HIGH durability (issue #36's fixed decision 3, this suite
/// pins salsa execution counts rather than stub behaviour) and a fixed
/// version range at MEDIUM. The file handle is kept so the pins can
/// edit it with `set_bytes`.
struct Harness {
    db: TestDatabase,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

fn single_file_harness(source: &[u8]) -> Harness {
    let db = TestDatabase::default();
    let file = SourceFile::new(&db, FileId::new(0), source.to_vec());
    let files = AnalyzedFileSet::new(&db, vec![file]);
    // Deliberately empty (issue #36's fixed decision 3): this suite pins
    // salsa execution counts, where a stub surface adds resolution
    // noise without observing stub behaviour, and a separate
    // compilation unit cannot reach `pub(crate)` test support anyway.
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    Harness {
        db,
        file,
        files,
        stubs,
        configuration,
    }
}

fn method_query<'db>(
    db: &'db TestDatabase,
    class_written: &str,
    member_written: &str,
) -> MemberQuery<'db> {
    MemberQuery::new(
        db,
        folded_symbol_key(SymbolSpace::ClassLike, class_written),
        MemberKind::Method,
        folded_member_key(MemberKind::Method, member_written),
    )
}

/// Pin 1: a method-body edit never reaches `declared_member_signature`.
/// The member tree is body-blind, so every input the declared signature
/// reads (the linearization, the item tree, the member tree) backdates
/// below it. The first computation drains the log and asserts the query
/// DID run, so the "not executed" assertion cannot pass vacuously.
#[test]
fn a_body_edit_never_recomputes_a_declared_signature() {
    let Harness {
        mut db,
        file,
        files,
        stubs,
        configuration,
    } = single_file_harness(b"<?php class C { public function f(): int { return 1; } }");

    {
        let query = method_query(&db, "C", "f");
        assert!(declared_member_signature(&db, files, stubs, configuration, query).is_some());
    }
    let seeded = db.take_executed();
    assert!(
        seeded
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "the declared signature must run on the first computation: {seeded:?}"
    );

    file.set_bytes(&mut db)
        .to(b"<?php class C { public function f(): int { return 2; } }".to_vec());

    {
        let query = method_query(&db, "C", "f");
        assert!(declared_member_signature(&db, files, stubs, configuration, query).is_some());
    }
    let executed = db.take_executed();
    assert!(
        !executed
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "a body edit must not reach the declared layer: {executed:?}"
    );
}

/// Pin 2: a docblock prose edit re-runs `declared_member_signature` (the
/// member payload carries the docblock text by design), but the result
/// is byte-identical, so a downstream verdict computed from the declared
/// return type is spared. This is the early cutoff the declared layer's
/// dependents live on: re-running the signature is cheap; re-running
/// every verdict that consulted it is not, and the identical output
/// keeps those verdicts memoized.
///
/// The probed verdict is routed through the class hierarchy on purpose:
/// `subtype_of` short-circuits to `Holds` on structural equality
/// (`candidate == target`) before it ever reads a salsa input, so a
/// scalar probe like `int <: int|string` would report "spared" even if
/// the declared layer's early cutoff were broken: it has no dependency
/// edge into file-tracked state to begin with. `User <: Entity` (two
/// distinct class names) instead reaches `judge_class_hierarchy`, which
/// reads `linearized_class` (see `a_body_edit_does_not_recompute_a_hierarchy_verdict`
/// above, which pins that same read directly). The verdict genuinely
/// depends on the file; "absent from `take_executed` after the edit" is
/// therefore load-bearing evidence of the early cutoff, not an artifact
/// of a closed-form check.
#[test]
fn a_docblock_prose_edit_recomputes_the_signature_but_spares_the_verdict() {
    let Harness {
        mut db,
        file,
        files,
        stubs,
        configuration,
    } = single_file_harness(
        b"<?php class Entity {} class User extends Entity {} \
          class Repo { /** initial */ public function find(): User {} }",
    );

    {
        let entity = TypeId::class(&db, "Entity", vec![]);
        let query = method_query(&db, "Repo", "find");
        let signature = declared_member_signature(&db, files, stubs, configuration, query).unwrap();
        assert_eq!(signature.value_type, TypeId::class(&db, "User", vec![]));
        assert_eq!(
            subtype_of(
                &db,
                files,
                stubs,
                configuration,
                signature.value_type,
                entity
            ),
            Proof::Holds
        );
    }
    let seeded = db.take_executed();
    assert!(
        seeded
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "the declared signature must run on the first computation: {seeded:?}"
    );
    assert!(
        seeded.iter().any(|query| query.contains("subtype_of")),
        "the dependent verdict must run on the first computation: {seeded:?}"
    );

    // Only the docblock prose changes; the signature text is untouched.
    file.set_bytes(&mut db)
        .to(b"<?php class Entity {} class User extends Entity {} \
          class Repo { /** changed prose */ public function find(): User {} }"
            .to_vec());

    {
        let entity = TypeId::class(&db, "Entity", vec![]);
        let query = method_query(&db, "Repo", "find");
        let signature = declared_member_signature(&db, files, stubs, configuration, query).unwrap();
        // The declared return is byte-identical (and interned-equal)
        // across the prose edit.
        assert_eq!(signature.value_type, TypeId::class(&db, "User", vec![]));
        assert_eq!(
            subtype_of(
                &db,
                files,
                stubs,
                configuration,
                signature.value_type,
                entity
            ),
            Proof::Holds
        );
    }
    let executed = db.take_executed();
    assert!(
        executed
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "a docblock prose edit reaches the declared signature by design: {executed:?}"
    );
    assert!(
        !executed.iter().any(|query| query.contains("subtype_of")),
        "an identical declared return must spare the dependent verdict: {executed:?}"
    );
}

/// Pin 3: a return-type signature edit (`int` -> `string`) re-runs the
/// query AND changes both its answer and every dependent verdict. The
/// counterexample to pin 2: here the declared output genuinely changes,
/// so the memoized verdict is invalidated and its answer flips from
/// Holds to Fails.
#[test]
fn a_return_type_edit_recomputes_the_signature_and_flips_the_verdict() {
    let Harness {
        mut db,
        file,
        files,
        stubs,
        configuration,
    } = single_file_harness(b"<?php class C { public function f(): int { return 1; } }");

    {
        let query = method_query(&db, "C", "f");
        let signature = declared_member_signature(&db, files, stubs, configuration, query).unwrap();
        assert_eq!(signature.value_type, TypeId::int(&db));
        assert_eq!(
            subtype_of(
                &db,
                files,
                stubs,
                configuration,
                signature.value_type,
                TypeId::int(&db)
            ),
            Proof::Holds
        );
    }
    let seeded = db.take_executed();
    assert!(
        seeded
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "the declared signature must run on the first computation: {seeded:?}"
    );
    assert!(
        seeded.iter().any(|query| query.contains("subtype_of")),
        "the dependent verdict must run on the first computation: {seeded:?}"
    );

    file.set_bytes(&mut db)
        .to(b"<?php class C { public function f(): string { return \"a\"; } }".to_vec());

    {
        let query = method_query(&db, "C", "f");
        let signature = declared_member_signature(&db, files, stubs, configuration, query).unwrap();
        // The answer changed: the declared return is now `string`.
        assert_eq!(signature.value_type, TypeId::string(&db));
        // A `string` return is not a subtype of `int`: the verdict flips.
        assert_eq!(
            subtype_of(
                &db,
                files,
                stubs,
                configuration,
                signature.value_type,
                TypeId::int(&db)
            ),
            Proof::Fails
        );
    }
    let executed = db.take_executed();
    assert!(
        executed
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "a return-type edit must re-run the declared signature: {executed:?}"
    );
    assert!(
        executed.iter().any(|query| query.contains("subtype_of")),
        "a changed declared return must re-run the dependent verdict: {executed:?}"
    );
}

/// Pin 4: a default-value edit (`= null` -> `= 1`) changes the declared
/// parameter's implicit nullability (design section 2, extended to the
/// declared level), which is part of the signature projection, so the
/// dependent verdict re-runs. `int $x = null` lowers to `int|null`;
/// `int $x = 1` lowers to `int`.
#[test]
fn a_default_value_edit_changes_parameter_nullability_and_reruns_the_verdict() {
    let Harness {
        mut db,
        file,
        files,
        stubs,
        configuration,
    } = single_file_harness(b"<?php class C { public function f(int $x = null) {} }");

    {
        let query = method_query(&db, "C", "f");
        let signature = declared_member_signature(&db, files, stubs, configuration, query).unwrap();
        let nullable_int = TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)]);
        assert_eq!(signature.parameters[0].parameter_type, Some(nullable_int));
        // `int|null` is not a subtype of `int`: the null admits refutation.
        assert_eq!(
            subtype_of(
                &db,
                files,
                stubs,
                configuration,
                nullable_int,
                TypeId::int(&db)
            ),
            Proof::Fails
        );
    }
    let seeded = db.take_executed();
    assert!(
        seeded
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "the declared signature must run on the first computation: {seeded:?}"
    );
    assert!(
        seeded.iter().any(|query| query.contains("subtype_of")),
        "the dependent verdict must run on the first computation: {seeded:?}"
    );

    file.set_bytes(&mut db)
        .to(b"<?php class C { public function f(int $x = 1) {} }".to_vec());

    {
        let query = method_query(&db, "C", "f");
        let signature = declared_member_signature(&db, files, stubs, configuration, query).unwrap();
        // The `= 1` default drops the implicit null: the parameter is now `int`.
        assert_eq!(
            signature.parameters[0].parameter_type,
            Some(TypeId::int(&db))
        );
        // The verdict flips to Holds, and because the argument type changed
        // it is a fresh judgment: the dependent re-runs.
        assert_eq!(
            subtype_of(
                &db,
                files,
                stubs,
                configuration,
                TypeId::int(&db),
                TypeId::int(&db)
            ),
            Proof::Holds
        );
    }
    let executed = db.take_executed();
    assert!(
        executed.iter().any(|query| query.contains("subtype_of")),
        "a changed declared parameter type must re-run the dependent verdict: {executed:?}"
    );
}

/// Pin 5: editing an UNRELATED member's signature in the same class
/// spares a verdict that depends on this member's declared signature.
/// Editing `g`'s return type mutates the linearized member table, so
/// `declared_member_signature(f)` re-runs (its ancestor walk reads the
/// linearization, an expected and name-level dependency). But `f`'s
/// declared return is byte-identical, so the dependent verdict keyed on
/// it stays memoized. The strongest true fact here is the SPARED verdict:
/// `take_executed` reports `declared_member_signature` re-running (the
/// linearization changed) but never `subtype_of`.
///
/// As in pin 2, `f`'s probed verdict is routed through the class
/// hierarchy (`User <: Entity`) rather than a structurally-equal scalar
/// check, so it carries a real dependency edge into `linearized_class`
/// and "absent from `take_executed`" is load-bearing rather than
/// vacuously true.
#[test]
fn an_unrelated_member_signature_edit_spares_the_other_members_verdict() {
    let Harness {
        mut db,
        file,
        files,
        stubs,
        configuration,
    } = single_file_harness(
        b"<?php class Entity {} class User extends Entity {} \
          class C { public function f(): User {} public function g(): int {} }",
    );

    {
        let entity = TypeId::class(&db, "Entity", vec![]);
        let query = method_query(&db, "C", "f");
        let signature = declared_member_signature(&db, files, stubs, configuration, query).unwrap();
        assert_eq!(signature.value_type, TypeId::class(&db, "User", vec![]));
        assert_eq!(
            subtype_of(
                &db,
                files,
                stubs,
                configuration,
                signature.value_type,
                entity
            ),
            Proof::Holds
        );
    }
    let seeded = db.take_executed();
    assert!(
        seeded
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "the declared signature must run on the first computation: {seeded:?}"
    );
    assert!(
        seeded.iter().any(|query| query.contains("subtype_of")),
        "the dependent verdict must run on the first computation: {seeded:?}"
    );

    // Only `g`'s return type changes; `f` is untouched.
    file.set_bytes(&mut db)
        .to(b"<?php class Entity {} class User extends Entity {} \
          class C { public function f(): User {} public function g(): string {} }"
            .to_vec());

    {
        let entity = TypeId::class(&db, "Entity", vec![]);
        let query = method_query(&db, "C", "f");
        let signature = declared_member_signature(&db, files, stubs, configuration, query).unwrap();
        // `f`'s declared return is unchanged by an edit to `g`.
        assert_eq!(signature.value_type, TypeId::class(&db, "User", vec![]));
        assert_eq!(
            subtype_of(
                &db,
                files,
                stubs,
                configuration,
                signature.value_type,
                entity
            ),
            Proof::Holds
        );
    }
    let executed = db.take_executed();
    assert!(
        executed
            .iter()
            .any(|query| query.contains("declared_member_signature")),
        "editing g must re-run f's declared signature: the linearization it \
         reads for the ancestor walk changed, by design: {executed:?}"
    );
    assert!(
        !executed.iter().any(|query| query.contains("subtype_of")),
        "an unrelated member edit must spare the other member's verdict: {executed:?}"
    );
}

/// A `TypeSyntax` fake that reads `@return <one word>`, ignoring every
/// other tag and every word of surrounding prose. The word lowers
/// through the shared native keyword table first (`site.keyword_type`)
/// and falls back to a class type qualified at the declaring site
/// (`site.qualify_class_name`) — the same two-step rule the native
/// lowering applies. A prose-only edit after the tag word therefore
/// reparses to the exact same `MemberAnnotations`; a changed tag word
/// reparses to a different one. Mirrors `FakeReturnSyntax` in
/// `crates/celerrate_types/src/declared.rs`'s test module, but reads
/// the tag's own word instead of answering a constant `int`.
#[derive(Debug)]
struct WordOnlyReturnSyntax;

impl TypeSyntax for WordOnlyReturnSyntax {
    fn can_parse(&self, docblock: &str) -> bool {
        docblock.contains("@return")
    }

    fn parse_docblock<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> ParsedAnnotations<'db> {
        let return_type = docblock
            .split_once("@return")
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .map(|word| {
                site.keyword_type(word).unwrap_or_else(|| {
                    TypeId::class(site.database(), &site.qualify_class_name(word), Vec::new())
                })
            });
        ParsedAnnotations {
            return_type,
            ..ParsedAnnotations::default()
        }
    }

    fn parse_type_expression<'db>(
        &self,
        _site: &AnnotationSite<'db, '_>,
        _expression: &str,
    ) -> Option<TypeId<'db>> {
        None
    }
}

fn fake_identity(name: &str) -> PluginIdentity {
    PluginIdentity {
        name: name.to_owned(),
        version: "0.0.0".to_owned(),
        configuration: String::new(),
    }
}

/// One project's inputs, with [`WordOnlyReturnSyntax`] registered at
/// HIGH durability before the first query runs — the two-stage cutoff
/// pins need annotations actually parsed through the registry, not the
/// no-plugin default every other fixture in this file exercises. File
/// handles are kept alongside so [`set_source`] can edit any one of
/// them.
struct AnnotationFixture {
    db: TestDatabase,
    handles: Vec<SourceFile>,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

fn fixture_with_fake_syntax(sources: &[&str]) -> AnnotationFixture {
    let db = TestDatabase::default();
    let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
        identity: fake_identity("fake-word-return"),
        implementation: Arc::new(WordOnlyReturnSyntax),
    }])
    .durability(salsa::Durability::HIGH)
    .new(&db);
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    // Deliberately empty (issue #36's fixed decision 3): this suite pins
    // salsa execution counts, where a stub surface adds resolution
    // noise without observing stub behaviour, and a separate
    // compilation unit cannot reach `pub(crate)` test support anyway.
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    AnnotationFixture {
        db,
        handles,
        files,
        stubs,
        configuration,
    }
}

/// Overwrites one file's bytes in place, the mutation every pin below
/// performs.
fn set_source(fixture: &mut AnnotationFixture, index: usize, source: &str) {
    fixture.handles[index]
        .set_bytes(&mut fixture.db)
        .to(source.as_bytes().to_vec());
}

/// A folded `MemberQuery` for one class-and-member pair, tied to the
/// database's own borrow rather than the whole fixture, so it can be
/// rebuilt around a `set_bytes` edit without holding a borrow live
/// across the mutation (the accommodation `method_query` above already
/// makes, generalized here over `MemberKind`).
fn member_query<'db>(
    db: &'db TestDatabase,
    class_written: &str,
    kind: MemberKind,
    member_written: &str,
) -> MemberQuery<'db> {
    MemberQuery::new(
        db,
        folded_symbol_key(SymbolSpace::ClassLike, class_written),
        kind,
        folded_member_key(kind, member_written),
    )
}

/// Pin 6 (task 7): the second-stage parsed-annotation cutoff, exercised
/// through an ACTUALLY REGISTERED, actually parsing `TypeSyntax`,
/// rather than the no-plugin default pin 2 (above) exercises.
///
/// A prose-only docblock edit changes the raw text, so stage one
/// (`member_tree`, read through `lookup_member`) re-runs — the spec's
/// accepted cost. `declared_member_signature` ALSO re-runs (asserted
/// below, deliberately, not zero): it independently calls
/// `lookup_member` for the member's own structural payload (kind,
/// name, native signature), exactly as pin 2 documents ("the member
/// payload carries the docblock text by design"), and that call sees
/// the same changed `Member` value regardless of what any registered
/// `TypeSyntax` does. `member_annotations` also re-runs, for the same
/// reason — but the fake syntax reads only the `@return` word, which
/// the added prose never touches, so it answers a byte-identical
/// `MemberAnnotations`, and the annotation-refined declared return is
/// the same interned `TypeId` as before the edit.
///
/// The probe is built FROM that declared return — `subtype_of(
/// signature.value_type, Entity)` — so its memoized verdict depends
/// transitively on what the seam produced, and it is hierarchy-routed
/// (`User <: Entity`, two distinct class names reaching
/// `linearized_class`) so "spared" cannot be a vacuous structural
/// short-circuit. The companion test below builds the IDENTICAL probe
/// construction after a tag edit and observes it re-run with a flipped
/// verdict, proving this probe family is sensitive to the seam's
/// output: its 0-execution result here is therefore load-bearing
/// evidence of the parsed-annotation cutoff, not coarse same-file
/// sparing.
#[test]
fn a_prose_only_docblock_edit_backdates_at_the_parsed_annotation_stage() {
    let mut fixture =
        fixture_with_fake_syntax(&["<?php class Entity {} class User extends Entity {} \
         class C { /** @return User */ public function f() {} }"]);
    {
        let query = member_query(&fixture.db, "C", MemberKind::Method, "f");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        // The annotation refines the native-`mixed` return to `User`.
        assert_eq!(
            signature.value_type,
            TypeId::class(&fixture.db, "User", vec![])
        );
        assert_eq!(
            subtype_of(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                signature.value_type,
                TypeId::class(&fixture.db, "Entity", vec![])
            ),
            Proof::Holds
        );
    }
    fixture.db.take_executed();

    set_source(
        &mut fixture,
        0,
        "<?php class Entity {} class User extends Entity {} \
         class C { /** @return User (documented better) */ public function f() {} }",
    );
    {
        let query = member_query(&fixture.db, "C", MemberKind::Method, "f");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        // The direct second-stage assertion: the parsed `@return User`
        // word is untouched by the prose, so the refined declared
        // return is the SAME interned type as before the edit.
        assert_eq!(
            signature.value_type,
            TypeId::class(&fixture.db, "User", vec![])
        );
        // The identical probe re-derived from the declared return: the
        // same interned key, and every hierarchy input it read
        // backdated, so the memoized verdict must answer without
        // executing.
        assert_eq!(
            subtype_of(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                signature.value_type,
                TypeId::class(&fixture.db, "Entity", vec![])
            ),
            Proof::Holds
        );
    }

    let log = fixture.db.take_executed();
    assert_eq!(executions_of(&log, "member_annotations"), 1, "{log:?}");
    assert_eq!(
        executions_of(&log, "declared_member_signature"),
        1,
        "declared_member_signature independently reads lookup_member for the \
         member's own structural payload, which carries the docblock text by \
         design (pin 2's accepted cost): it re-runs on every docblock edit, \
         prose or tag, regardless of the parsed-annotation cutoff: {log:?}"
    );
    assert_eq!(
        executions_of(&log, "subtype_of"),
        0,
        "the verdict derived from the declared return must stay spared: the \
         edit stops mattering at the parsed-annotation stage, not at \
         declared_member_signature's own re-execution: {log:?}"
    );
}

/// The counterexample, sharing the EXACT probe construction of the
/// prose pin above (`subtype_of(signature.value_type, Entity)`):
/// editing the `@return` word itself (`User` -> `Other`) changes what
/// the fake syntax extracts, so `member_annotations` produces a
/// genuinely different value, the refined declared return changes with
/// it, and the probe re-executes with a flipped verdict (`Other` does
/// not extend `Entity`). This proves the shared probe family is
/// sensitive to the seam's output — the prose pin's 0-execution result
/// discriminates the parsed-annotation cutoff, not same-file sparing.
#[test]
fn an_annotation_edit_reaches_the_declared_signature() {
    let mut fixture = fixture_with_fake_syntax(&[
        "<?php class Entity {} class User extends Entity {} class Other {} \
         class C { /** @return User */ public function f() {} }",
    ]);
    {
        let query = member_query(&fixture.db, "C", MemberKind::Method, "f");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        assert_eq!(
            signature.value_type,
            TypeId::class(&fixture.db, "User", vec![])
        );
        assert_eq!(
            subtype_of(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                signature.value_type,
                TypeId::class(&fixture.db, "Entity", vec![])
            ),
            Proof::Holds
        );
    }
    fixture.db.take_executed();

    set_source(
        &mut fixture,
        0,
        "<?php class Entity {} class User extends Entity {} class Other {} \
         class C { /** @return Other */ public function f() {} }",
    );
    {
        let query = member_query(&fixture.db, "C", MemberKind::Method, "f");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        // The declared return genuinely changed with the tag word.
        assert_eq!(
            signature.value_type,
            TypeId::class(&fixture.db, "Other", vec![])
        );
        // The same probe construction, now keyed on the new declared
        // return: it must execute, and its verdict flips.
        assert_eq!(
            subtype_of(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                signature.value_type,
                TypeId::class(&fixture.db, "Entity", vec![])
            ),
            Proof::Fails
        );
    }

    let log = fixture.db.take_executed();
    assert_eq!(executions_of(&log, "member_annotations"), 1, "{log:?}");
    assert_eq!(
        executions_of(&log, "declared_member_signature"),
        1,
        "{log:?}"
    );
    assert!(
        executions_of(&log, "subtype_of") >= 1,
        "a changed declared return must reach the dependent verdict: {log:?}"
    );
}

/// Pin 8 (task 9): `member_annotations` now ALSO reads the owner
/// class-like's OWN docblock (`owner_class_docblock`), to expose
/// class-level `@template` declarations while parsing a member's
/// annotations — unconditionally, for every registered `TypeSyntax`,
/// whether or not that implementation ever calls
/// `AnnotationSite::enclosing_class_docblock`. Before this task, a
/// CLASS-level docblock edit never reached `member_annotations`: its
/// only class-docblock-adjacent dependency was the owner's namespace
/// (via `declaring_site`), unaffected by prose. This pin proves the
/// new edge is real (`member_annotations` reruns once) yet inert for a
/// `TypeSyntax` that never reads the class docblock's tags (the fake
/// here reads only the member's own `@return` word), so the identical
/// downstream verdict stays memoized: an honest re-execution cost, not
/// undue churn.
#[test]
fn a_class_docblock_prose_edit_backdates_at_the_member_annotations_stage() {
    let mut fixture =
        fixture_with_fake_syntax(&["<?php class Entity {} class User extends Entity {} \
         /** class prose */ class C { /** @return User */ public function f() {} }"]);
    {
        let query = member_query(&fixture.db, "C", MemberKind::Method, "f");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        assert_eq!(
            signature.value_type,
            TypeId::class(&fixture.db, "User", vec![])
        );
        assert_eq!(
            subtype_of(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                signature.value_type,
                TypeId::class(&fixture.db, "Entity", vec![])
            ),
            Proof::Holds
        );
    }
    fixture.db.take_executed();

    // Only the CLASS docblock's prose changes; the member's own
    // docblock and signature are untouched.
    set_source(
        &mut fixture,
        0,
        "<?php class Entity {} class User extends Entity {} \
         /** class prose, reworded */ class C { /** @return User */ public function f() {} }",
    );
    {
        let query = member_query(&fixture.db, "C", MemberKind::Method, "f");
        let signature = declared_member_signature(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .unwrap();
        // The member's own parsed annotation is untouched by the class
        // docblock's prose: the refined declared return is the SAME
        // interned type as before the edit.
        assert_eq!(
            signature.value_type,
            TypeId::class(&fixture.db, "User", vec![])
        );
        // The identical probe re-derived from the declared return: the
        // memoized verdict must answer without executing.
        assert_eq!(
            subtype_of(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                signature.value_type,
                TypeId::class(&fixture.db, "Entity", vec![])
            ),
            Proof::Holds
        );
    }

    let log = fixture.db.take_executed();
    assert_eq!(
        executions_of(&log, "member_annotations"),
        1,
        "member_annotations now reads the class-like's own docblock \
         (for `@template` visibility), a new dependency edge a \
         class-level prose edit exercises: {log:?}"
    );
    assert_eq!(
        executions_of(&log, "subtype_of"),
        0,
        "an unaffected member annotation must spare the dependent verdict: {log:?}"
    );
}

/// One project's inputs for the inference-layer pins below, without any
/// registered `TypeSyntax` plugin: the design's harness-2 edit classes
/// reach native declared signatures and `inferred_body_types` directly,
/// not annotation parsing. File handles are kept so a pin can edit any
/// one of them with `set_bytes` (the same `AnnotationFixture`/
/// `Harness` shape above, generalized to the inference-layer queries).
struct InferenceFixture {
    db: TestDatabase,
    handles: Vec<SourceFile>,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

fn fixture(sources: &[&str]) -> InferenceFixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    // Deliberately empty (issue #36's fixed decision 3): this suite pins
    // salsa execution counts, where a stub surface adds resolution
    // noise without observing stub behaviour, and a separate
    // compilation unit cannot reach `pub(crate)` test support anyway.
    let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    InferenceFixture {
        db,
        handles,
        files,
        stubs,
        configuration,
    }
}

/// Overwrites one file's bytes in place (mirrors `set_source` above,
/// generalized over [`InferenceFixture`]).
fn set_inference_source(fixture: &mut InferenceFixture, index: usize, source: &str) {
    fixture.handles[index]
        .set_bytes(&mut fixture.db)
        .to(source.as_bytes().to_vec());
}

/// A folded `FunctionQuery` for a free function written in the
/// fixture's global namespace.
fn function_query<'db>(db: &'db TestDatabase, written: &str) -> FunctionQuery<'db> {
    FunctionQuery::new(db, folded_symbol_key(SymbolSpace::Function, written))
}

/// The display of a free function's inferred return, resolved through
/// `inferred_function_return` by its folded key (task 13's closing
/// pin needs a caller, mirroring the crate's own `caller_return_display`
/// test helper in `inference.rs`).
fn caller_return_display(fixture: &InferenceFixture, key: &str) -> String {
    inferred_function_return(
        &fixture.db,
        fixture.files,
        fixture.stubs,
        fixture.configuration,
        function_query(&fixture.db, key),
    )
    .display(&fixture.db)
}

/// Harness-2, pin 1: a callee body edit that leaves the inferred
/// return identical spares the caller. The callee re-infers (its body
/// genuinely changed), but the joined return type does not, so
/// `inferred_function_return`'s early cutoff backdates and the
/// caller's own inference is never re-demanded transitively (design
/// section 10, harness 2).
#[test]
fn a_body_edit_with_an_identical_inferred_return_spares_callers() {
    // File 0: the caller. File 1: the callee.
    let mut fixture = fixture(&[
        "<?php function caller() { return callee(); }",
        "<?php function callee() { return 1; }",
    ]);
    {
        let caller = function_query(&fixture.db, "caller");
        let _ = inferred_function_return(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            caller,
        );
    }
    let _ = fixture.db.take_executed();

    // The callee's body changes; its inferred return does not.
    set_inference_source(
        &mut fixture,
        1,
        "<?php function callee() { $noise = 'x'; return 1; }",
    );

    {
        let caller = function_query(&fixture.db, "caller");
        let _ = inferred_function_return(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            caller,
        );
    }
    let log = fixture.db.take_executed();
    // The callee re-infers; the identical return backdates; the
    // caller's inference never re-runs (design section 10, harness 2).
    assert_eq!(executions_of(&log, "inferred_body_types"), 1, "{log:?}");
}

/// Harness-2, pin 2: a prose-only docblock edit on the callee re-runs
/// no typed inference at all. Unlike the declared layer's own prose
/// pin (which re-runs `declared_member_signature` and stops there),
/// the inference layer never reads the docblock text, so the body IR
/// and every input `inferred_body_types` consults are untouched, and
/// the query is never re-demanded on the caller's account.
#[test]
fn a_prose_docblock_edit_re_runs_no_inference() {
    let mut fixture = fixture(&[
        "<?php function caller() { return callee(); }",
        "<?php /** a docblock */ function callee(): int { return 1; }",
    ]);
    {
        let caller = function_query(&fixture.db, "caller");
        let _ = inferred_function_return(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            caller,
        );
    }
    let _ = fixture.db.take_executed();

    set_inference_source(
        &mut fixture,
        1,
        "<?php /** reworded prose */ function callee(): int { return 1; }",
    );

    {
        let caller = function_query(&fixture.db, "caller");
        let _ = inferred_function_return(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            caller,
        );
    }
    let log = fixture.db.take_executed();
    // The two-stage cutoff (design section 5): the annotation parse
    // re-runs and backdates; no typed query above it re-executes.
    assert_eq!(executions_of(&log, "inferred_body_types"), 0, "{log:?}");
}

/// Harness-2, pin 3: a default-value edit (`= null` -> `= 'd'`) is
/// part of the comparable signature (the 1a contract): the seeded
/// parameter type the body's inference reads from changes, so the
/// member projection changes and the body genuinely re-infers.
#[test]
fn a_default_value_edit_invalidates_the_signatures_dependents() {
    let mut fixture = fixture(&["<?php function callee(?string $s = null) { return $s; }"]);
    {
        let callee = function_query(&fixture.db, "callee");
        let _ = inferred_function_return(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            callee,
        );
    }
    let _ = fixture.db.take_executed();

    set_inference_source(
        &mut fixture,
        0,
        "<?php function callee(?string $s = 'd') { return $s; }",
    );

    {
        let callee = function_query(&fixture.db, "callee");
        let _ = inferred_function_return(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            callee,
        );
    }
    let log = fixture.db.take_executed();
    // The default value is part of the comparable signature (the 1a
    // contract): the member projection changed, so the body re-infers.
    assert_eq!(executions_of(&log, "inferred_body_types"), 1, "{log:?}");
}

/// Demands the inferred return of both of class `A`'s methods through
/// the method-inferred tier (plan 6's `inferred_method_return`), the
/// path a real caller takes.
fn demand_method_returns(fixture: &InferenceFixture) {
    for name in ["edited", "bystander"] {
        let _ = inferred_method_return(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            MethodQuery::new(&fixture.db, "a".to_owned(), name.to_owned()),
        );
    }
}

/// Harness-2, pin 4: editing one method's signature in a class spares
/// every sibling member's body inference. `member_tree` changes (the
/// whole class's member table is one query), but the per-body
/// `body_owner` projection backdates for every body whose own
/// declaration did not, so only the edited member's body re-infers
/// (its parameter seed changed) and the bystander is spared (design
/// section 10, harness 2).
#[test]
fn editing_one_signature_spares_the_other_members_inference() {
    let source_before = "<?php class A {
        public function edited(int $n) { return $n; }
        public function bystander() { return 'x'; }
    }";
    let source_after = "<?php class A {
        public function edited(string $n) { return $n; }
        public function bystander() { return 'x'; }
    }";
    let mut fixture = fixture(&[source_before]);
    // Plan 6 landed the method-inferred tier, so the demand runs
    // through `inferred_method_return` — the path a caller actually
    // takes — rather than reaching for each body identity directly.
    // The scenario and its contract are unchanged.
    demand_method_returns(&fixture);
    let _ = fixture.db.take_executed();

    set_inference_source(&mut fixture, 0, source_after);

    demand_method_returns(&fixture);
    let log = fixture.db.take_executed();
    // `member_tree` changed, but the per-body `body_owner` projection
    // backdates for every body whose own declaration did not: only
    // `edited`'s body re-infers (its parameter seed changed);
    // `bystander` is spared. This is the design's "editing one
    // signature does not invalidate other members' bodies" contract
    // (section 10, harness 2).
    assert_eq!(executions_of(&log, "inferred_body_types"), 1, "{log:?}");
}

/// Task 13's closing pin, harness-2 extended to a method callee: a
/// callee METHOD body edit that leaves the inferred return identical
/// spares the caller, exactly like the free-function pin above, but
/// through `inferred_method_return` rather than `inferred_body_types`
/// directly — the path a real cross-class call takes.
#[test]
fn a_callee_body_edit_with_an_identical_inferred_return_backdates_callers() {
    let before = r#"<?php
namespace App;
class Greeter {
    public function greeting() { $word = 'hello'; return $word; }
}
function caller(Greeter $greeter) { return $greeter->greeting(); }
"#;
    // The local is renamed: the body IR changes, the callee re-infers,
    // but the inferred return is identical — early cutoff on
    // inferred_method_return spares the caller.
    let after = before.replace("$word", "$greeting");
    let mut f = fixture(&[before]);
    let _ = caller_return_display(&f, "app\\caller");
    f.db.take_executed();
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    let _ = caller_return_display(&f, "app\\caller");
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "inferred_body_types"),
        1,
        "only the edited callee re-infers: {log:?}",
    );
}

/// Task 13's closing pin: a trait body edit reaches every using
/// class's own body inference (each using class analyzes the trait's
/// body per its own context, decision 5/task 7) and only them — the
/// bystander class that never uses the trait is untouched.
#[test]
fn a_trait_body_edit_reaches_each_using_class_and_only_them() {
    let before = r#"<?php
namespace App;
trait Reader {
    public function read() { return $this->value; }
}
class IntBox { use Reader; public int $value = 0; }
class StringBox { use Reader; public string $value = ''; }
class Unrelated {
    public function noop() { return 1; }
}
"#;
    let after = before.replace("return $this->value;", "return $this->value ?? null;");
    let mut f = fixture(&[before]);
    for class in ["app\\intbox", "app\\stringbox", "app\\unrelated"] {
        let member = if class == "app\\unrelated" {
            "noop"
        } else {
            "read"
        };
        let _ = inferred_method_return(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            MethodQuery::new(&f.db, class.to_owned(), member.to_owned()),
        );
    }
    f.db.take_executed();
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    for class in ["app\\intbox", "app\\stringbox", "app\\unrelated"] {
        let member = if class == "app\\unrelated" {
            "noop"
        } else {
            "read"
        };
        let _ = inferred_method_return(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            MethodQuery::new(&f.db, class.to_owned(), member.to_owned()),
        );
    }
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "inferred_body_types"),
        2,
        "one re-inference per using class, none for the bystander: {log:?}",
    );
}

/// A fake `TypeSyntax` supporting exactly the notation the two
/// class-annotation pins below exercise: `@template NAME` (bare,
/// unbounded), `@extends NAME<ARG, ...>`, and `@return NAME` — a bare
/// name that resolves to a template visible either in the docblock
/// currently being parsed (a class's own `@extends` argument, read
/// against its own `@template` list) or in the enclosing class's
/// docblock (a member's `@return`, task 9's `enclosing_class_docblock`
/// exposure), and otherwise to a class qualified at the site.
/// Deliberately a second, smaller copy of
/// `inheritance::test_support::FakeSyntax`'s essential notation: that
/// module is `#[cfg(test)]`-private to the library crate and
/// unreachable from this external integration-test binary, and the
/// crate has no shared test-support module spanning the crate boundary
/// (a recorded debt `test_support.rs`'s own module doc already
/// names).
#[derive(Debug)]
struct InheritanceFakeSyntax;

impl InheritanceFakeSyntax {
    /// Strips a docblock's `/**`/`*/` delimiters and each line's
    /// leading `*`, into one plain-text line per tag — copes with both
    /// the single-line and multi-line conventions.
    fn docblock_lines(docblock: &str) -> Vec<String> {
        let text = docblock.trim();
        let text = text.strip_prefix("/**").unwrap_or(text);
        let text = text.strip_suffix("*/").unwrap_or(text);
        text.lines()
            .map(|line| line.trim().trim_start_matches('*').trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect()
    }

    /// The declared names of every `@template` tag in `docblock`, in
    /// declaration order.
    fn template_names(docblock: &str) -> Vec<String> {
        Self::docblock_lines(docblock)
            .iter()
            .filter_map(|line| line.strip_prefix("@template "))
            .map(|rest| rest.trim().to_owned())
            .collect()
    }

    /// A written name: a template declared on the enclosing class (a
    /// member docblock's own scope) lowers scoped to that class — the
    /// scope `ancestor_substitution` fixes against; a template declared
    /// in `own_templates` (the docblock currently being parsed, for a
    /// class's own `@extends` argument) lowers scoped to this site's
    /// own declaring scope; anything else is a class qualified at the
    /// site.
    fn lower_name<'db>(
        site: &AnnotationSite<'db, '_>,
        own_templates: &[String],
        written: &str,
    ) -> TypeId<'db> {
        let db = site.database();
        let enclosing: Vec<String> = site
            .enclosing_class_docblock()
            .map(Self::template_names)
            .unwrap_or_default();
        if enclosing.iter().any(|name| name == written) {
            let scope = site.enclosing_class_scope().unwrap_or("");
            return TypeId::template(db, scope, written, TypeId::mixed(db));
        }
        if own_templates.iter().any(|name| name == written) {
            return TypeId::template(db, site.declaring_scope(), written, TypeId::mixed(db));
        }
        if let Some(keyword) = site.keyword_type(written) {
            return keyword;
        }
        TypeId::class(db, &site.qualify_class_name(written).to_lowercase(), vec![])
    }
}

impl TypeSyntax for InheritanceFakeSyntax {
    fn can_parse(&self, docblock: &str) -> bool {
        docblock.contains("@template")
            || docblock.contains("@extends")
            || docblock.contains("@return")
    }

    fn parse_docblock<'db>(
        &self,
        site: &AnnotationSite<'db, '_>,
        docblock: &str,
    ) -> ParsedAnnotations<'db> {
        let own_templates = Self::template_names(docblock);
        let mut templates = Vec::new();
        let mut ancestors = Vec::new();
        let mut return_type = None;
        for line in Self::docblock_lines(docblock) {
            if let Some(rest) = line.strip_prefix("@template ") {
                templates.push(ParsedTemplate {
                    name: rest.trim().to_owned(),
                    bound: None,
                });
            } else if let Some(rest) = line.strip_prefix("@extends ") {
                if let Some((head, tail)) = rest.split_once('<')
                    && let Some(arguments_text) = tail.strip_suffix('>')
                {
                    let class_name = site.qualify_class_name(head.trim()).to_lowercase();
                    let arguments = arguments_text
                        .split(',')
                        .map(|argument| Self::lower_name(site, &own_templates, argument.trim()))
                        .collect();
                    ancestors.push(ParsedAncestor {
                        class_name,
                        arguments,
                    });
                }
            } else if let Some(rest) = line.strip_prefix("@return ") {
                return_type = Some(Self::lower_name(site, &own_templates, rest.trim()));
            }
        }
        ParsedAnnotations {
            templates,
            ancestors,
            return_type,
            ..ParsedAnnotations::default()
        }
    }

    fn parse_type_expression<'db>(
        &self,
        _site: &AnnotationSite<'db, '_>,
        _expression: &str,
    ) -> Option<TypeId<'db>> {
        None
    }
}

/// `fixture` plus [`InheritanceFakeSyntax`] registered at HIGH
/// durability: an isolated variant (mirroring `inference.rs`'s own
/// `fixture_with_generics`) rather than a change to the shared
/// `fixture` — registering the fake globally would give every existing
/// docblock-bearing test in this module a different annotation
/// reading.
fn fixture_with_inheritance_syntax(sources: &[&str]) -> InferenceFixture {
    let built = fixture(sources);
    let _ = TypeSyntaxRegistry::builder(vec![TypeSyntaxRegistration {
        identity: fake_identity("fake-inheritance"),
        implementation: Arc::new(InheritanceFakeSyntax),
    }])
    .durability(salsa::Durability::HIGH)
    .new(&built.db);
    built
}

/// Task 13's closing pin: a prose-only edit to a CLASS's own docblock
/// (the `@template` declaration site) is a genuine cutoff hole flushed
/// by this probe (Step 3's own anticipated outcome), just not the one
/// the brief named. `class_annotations` re-runs (its
/// `owner_class_docblock` input's text changed) and produces a
/// byte-identical parsed template list — but `declared_member_signature`
/// ALSO re-runs, unconditionally. `declared_member_signature` has two
/// separate reads that reach the same file-granular `member_tree` —
/// bundling every class's own docblock text into one
/// `Vec<ClassMembers>` — but only one of them lacks a memo boundary
/// for THIS edit: its direct call to the plain (non-tracked)
/// `declaring_site`/`owner_class_docblock` helpers (`declared.rs:444`),
/// which read `member_tree` with nothing memoized in between, so
/// Repository's changed docblock forces a re-execution
/// unconditionally. Its other read, `lookup_member`
/// (`declared.rs:382`), reaches `member_tree` too, transitively
/// through `linearized_class`'s own `fetch` helper
/// (`linearize.rs:758`) — but `linearized_class` IS a tracked query,
/// so when Repository's changed docblock leaves `UserRepository`'s own
/// linearized member list byte-identical, that query's early cutoff
/// absorbs the change before `lookup_member` is ever re-invoked
/// (confirmed empirically: instrumenting the log shows `lookup_member`
/// executing zero times across this edit). So the direct helper calls,
/// not `lookup_member`, are the forcing mechanism for THIS pin's edit
/// — an honest, already-documented cost (the sibling pin
/// `a_docblock_prose_edit_recomputes_the_signature_but_spares_the_verdict`
/// names `lookup_member` for a DIFFERENT edit, the queried member's OWN
/// docblock, where `lookup_member`'s own returned payload genuinely
/// changes; this pin shows a second, independent file-granular cost
/// reaching any OTHER class's docblock in the same file). The
/// two-stage cutoff this pin can actually demonstrate is one layer
/// further down: the refined VALUE stays identical, so a downstream
/// verdict computed from it — routed through the class hierarchy
/// exactly as the sibling pins insist on, so "spared" is not a vacuous
/// structural short-circuit — stays memoized.
#[test]
fn a_prose_only_class_docblock_edit_recomputes_the_signature_but_spares_the_verdict() {
    let before = r#"<?php
namespace App;
class Entity {}
/**
 * The repository.
 * @template T
 */
class Repository {
    /** @return T */
    public function find(int $identifier) {}
}
/** @extends Repository<User> */
class UserRepository extends Repository {}
class User extends Entity {}
"#;
    let after = before.replace("The repository.", "The repository, but described better.");
    let mut f = fixture_with_inheritance_syntax(&[before]);
    let query = MemberQuery::new(
        &f.db,
        "app\\userrepository".to_owned(),
        MemberKind::Method,
        "find".to_owned(),
    );
    let entity = TypeId::class(&f.db, "app\\entity", vec![]);
    {
        let signature =
            declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query).unwrap();
        assert_eq!(
            signature.value_type,
            TypeId::class(&f.db, "app\\user", vec![])
        );
        assert_eq!(
            subtype_of(
                &f.db,
                f.files,
                f.stubs,
                f.configuration,
                signature.value_type,
                entity
            ),
            Proof::Holds
        );
    }
    f.db.take_executed();
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    // Rebuilt: `MemberQuery`'s interned lifetime ties it to the borrow
    // of `f.db` at construction, which the mutable `set_bytes` borrow
    // above ends — a fresh, identically-keyed query is the same salsa
    // interned id either way (same class/kind/member text).
    let query = MemberQuery::new(
        &f.db,
        "app\\userrepository".to_owned(),
        MemberKind::Method,
        "find".to_owned(),
    );
    let entity = TypeId::class(&f.db, "app\\entity", vec![]);
    {
        let signature =
            declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query).unwrap();
        // The refined return is byte-identical across the prose edit.
        assert_eq!(
            signature.value_type,
            TypeId::class(&f.db, "app\\user", vec![])
        );
        assert_eq!(
            subtype_of(
                &f.db,
                f.files,
                f.stubs,
                f.configuration,
                signature.value_type,
                entity
            ),
            Proof::Holds
        );
    }
    let log = f.db.take_executed();
    assert!(
        executions_of(&log, "class_annotations") >= 1,
        "the annotation parse re-runs over the edited docblock: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "declared_member_signature"),
        1,
        "declared_member_signature re-runs unconditionally on ANY docblock \
         edit in the file: its direct calls to the plain (non-tracked) \
         declaring_site/owner_class_docblock helpers (declared.rs:444) read \
         the file-granular member_tree with no memo boundary in between. \
         Its other file-granular read, lookup_member (declared.rs:382), \
         reaches the same member_tree transitively through \
         linearized_class, but that query IS tracked and backdates here \
         (lookup_member executes 0 times across this edit), so it is not \
         the forcing mechanism for this particular edit — an honest, \
         already-documented cost, not a defect this task fixes: {log:?}",
    );
    assert_eq!(
        executions_of(&log, "subtype_of"),
        0,
        "the identical refined value spares the downstream verdict — the \
         two-stage cutoff this pin can actually demonstrate: {log:?}",
    );
}

/// Task 13's closing pin, the counterexample to the pin above: editing
/// the `@extends` argument itself (`Repository<User>` ->
/// `Repository<Admin>`) genuinely changes `class_annotations`'s parsed
/// ancestors, so the threaded argument changes with it and the next
/// demand answers the new class.
#[test]
fn an_extends_argument_edit_invalidates_inherited_signature_dependents() {
    let before = r#"<?php
namespace App;
/** @template T */
class Repository {
    /** @return T */
    public function find(int $identifier) {}
}
/** @extends Repository<User> */
class UserRepository extends Repository {}
class User {}
class Admin {}
"#;
    let after = before.replace("Repository<User>", "Repository<Admin>");
    let mut f = fixture_with_inheritance_syntax(&[before]);
    let query = MemberQuery::new(
        &f.db,
        "app\\userrepository".to_owned(),
        MemberKind::Method,
        "find".to_owned(),
    );
    let first = declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query)
        .unwrap()
        .value_type;
    assert_eq!(first, TypeId::class(&f.db, "app\\user", vec![]));
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    // Rebuilt for the same reason as the pin above: a fresh,
    // identically-keyed query outlives the `set_bytes` mutable borrow.
    let query = MemberQuery::new(
        &f.db,
        "app\\userrepository".to_owned(),
        MemberKind::Method,
        "find".to_owned(),
    );
    let second = declared_member_signature(&f.db, f.files, f.stubs, f.configuration, query)
        .unwrap()
        .value_type;
    assert_eq!(
        second,
        TypeId::class(&f.db, "app\\admin", vec![]),
        "the threaded argument flows through on the next demand",
    );
}

/// `fixture` with the real embedded stub blob and `StdlibProvider`
/// registered (task 6's registration idiom; duplicated from
/// `tests/fixpoint.rs`'s identical helper — no shared test-support
/// module spans this crate's integration-test binaries, the same
/// constraint `executions_of` above already notes). Task 12's closing
/// pin needs a provider whose answer is a genuine pure function of the
/// `Invocation` (decision 16), not the empty stub index the rest of
/// this suite uses to keep resolution noise out.
fn fixture_with_embedded_stubs_and_stdlib_provider(sources: &[&str]) -> InferenceFixture {
    let db = TestDatabase::default();
    let handles: Vec<SourceFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
        })
        .collect();
    let files = AnalyzedFileSet::new(&db, handles.clone());
    let stubs = StubIndexInput::builder(celerrate_stubs::embedded_stub_index().unwrap())
        .durability(salsa::Durability::HIGH)
        .new(&db);
    let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
        PhpVersion::new(8, 1),
        PhpVersion::new(8, 5),
    ))
    .durability(salsa::Durability::MEDIUM)
    .new(&db);
    let _ = DynamicTypeProviderRegistry::builder(vec![DynamicTypeProviderRegistration {
        identity: celerrate_stdlib_provider::descriptor().identity,
        provider: Arc::new(StdlibProvider::new()),
    }])
    .durability(salsa::Durability::HIGH)
    .new(&db);
    InferenceFixture {
        db,
        handles,
        files,
        stubs,
        configuration,
    }
}

/// The body of the declaration numbered `index` in file 0 (mirrors
/// `inference.rs`'s own private `body_query` test helper, unreachable
/// from this external integration-test binary, and `tests/fixpoint.rs`'s
/// identical duplicate).
fn body_query(fixture: &InferenceFixture, index: u32) -> BodyQuery<'_> {
    BodyQuery::new(
        &fixture.db,
        AstId {
            file: FileId::new(0),
            index,
        },
    )
}

/// Task 12's load-bearing invalidation pin (decision 16): "a provider
/// answer changes only when its `Invocation` changes." A provider's
/// answer is a pure function of the argument types it is handed
/// (`json_decode`'s second argument here, an `Invocation` component),
/// so editing an argument LITERAL re-infers only the body containing
/// the edit, never a sibling body in the same file that never
/// consulted the provider. `bystander` calls `strlen`, a function the
/// provider does not claim at all, so it has no dependency on the
/// provider's answer either way — its non-re-execution is not merely
/// "same file, spared" but "no edge to the edited call at all". The
/// counterexample proving the 0/1 split is not vacuous: `bystander`
/// alone (no `decoding`) is exercised by a sibling pin's identical
/// shape (`a_prose_docblock_edit_re_runs_no_inference`, expecting 0)
/// and every earlier pin in this suite already establishes that an
/// edited file's OTHER declaration re-infers when its own dependency
/// changed (`editing_one_signature_spares_the_other_members_inference`),
/// so a "both bodies always re-run on any edit in the file" alternative
/// implementation would fail this assertion's `== 1`.
#[test]
fn an_argument_literal_edit_reruns_only_the_editing_body() {
    // A provider answer is a pure function of the invocation, so an
    // argument-literal edit moves nothing but the editing body.
    let before = r#"<?php
function decoding(string $json) { return json_decode($json, true); }
function bystander(string $text) { return strlen($text); }
"#;
    let after = before.replace(", true)", ", false)");
    let mut f = fixture_with_embedded_stubs_and_stdlib_provider(&[before]);
    let file = f.handles[0];
    for index in [0, 1] {
        let _ = inferred_body_types(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            file,
            body_query(&f, index),
            InferenceContext::new(&f.db, None),
        );
    }
    f.db.take_executed();
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    for index in [0, 1] {
        let _ = inferred_body_types(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            file,
            body_query(&f, index),
            InferenceContext::new(&f.db, None),
        );
    }
    let log = f.db.take_executed();
    assert_eq!(
        executions_of(&log, "inferred_body_types"),
        1,
        "only the editing body re-infers: {log:?}",
    );
}

/// Task 12's second pin (decision 16): a provider answer, once it
/// changes, propagates through the ordinary interprocedural path —
/// nothing special-cased. `json_decode`'s `$associative` literal edit
/// flips `decoding`'s inferred return from the array branch to the
/// object branch (`json_functions.rs`'s truth table), and `caller`,
/// which never mentions `json_decode` itself, still sees the flip
/// through `inferred_function_return`'s ordinary callee-return
/// dependency — the same path a declared-return or inferred-return
/// edge would ride.
#[test]
fn a_provider_answer_change_propagates_like_any_inferred_return() {
    // The flags edit changes the callee's inferred return; the
    // caller re-infers on demand — provider answers ride the
    // existing invalidation paths, no special casing.
    let before = r#"<?php
function decoding(string $json) { return json_decode($json, true); }
function caller(string $json) { return decoding($json); }
"#;
    let after = before.replace(", true)", ", false)");
    let mut f = fixture_with_embedded_stubs_and_stdlib_provider(&[before]);
    let first = inferred_function_return(
        &f.db,
        f.files,
        f.stubs,
        f.configuration,
        FunctionQuery::new(&f.db, "caller".to_owned()),
    )
    .display(&f.db);
    let handle = f.handles.first().copied().unwrap();
    handle.set_bytes(&mut f.db).to(after.into_bytes());
    let second = inferred_function_return(
        &f.db,
        f.files,
        f.stubs,
        f.configuration,
        FunctionQuery::new(&f.db, "caller".to_owned()),
    )
    .display(&f.db);
    assert_ne!(
        first, second,
        "the array branch became the object branch through the caller",
    );
}
