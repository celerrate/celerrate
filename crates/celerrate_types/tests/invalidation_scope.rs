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

#![allow(clippy::unwrap_used)]
#![allow(clippy::indexing_slicing)]

use std::sync::Arc;

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    MemberKind, MemberQuery, PluginIdentity, SymbolSpace, folded_member_key, folded_symbol_key,
};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{
    AnnotationSite, ParsedAnnotations, Proof, TypeId, TypeSyntax, TypeSyntaxRegistration,
    TypeSyntaxRegistry, declared_member_signature, subtype_of,
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

/// One source file plus the whole-project inputs, in the exact shape the
/// declared layer's own tests build (an empty stub index at HIGH
/// durability, a fixed version range at MEDIUM). The file handle is kept
/// so the pins can edit it with `set_bytes`.
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
