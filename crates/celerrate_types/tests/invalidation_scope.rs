//! The typed judgments must ride the member boundary's early cutoff: a
//! method-body edit backdates the member tree, so a memoized subtype
//! verdict that consulted the hierarchy does not recompute.

#![allow(clippy::unwrap_used)]
#![allow(clippy::indexing_slicing)]

use celerrate_db::testing::TestDatabase;
use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::{
    MemberKind, MemberQuery, SymbolSpace, folded_member_key, folded_symbol_key,
};
use celerrate_source::FileId;
use celerrate_stubs::{StubIndex, StubIndexInput};
use celerrate_types::{Proof, TypeId, declared_member_signature, subtype_of};
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
