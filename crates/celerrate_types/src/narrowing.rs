//! Narrowing subjects (design section 6: locals, and property fetches
//! on a stable base — `$this->prop`, `self::$prop`) and, from Task 4
//! on, the pure leaf transformations the condition forms reduce to.

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{BodyExpression, BodyIr, ExpressionId, MemberReference};
use celerrate_stubs::StubIndexInput;

use crate::judgments::{Proof, subtype_of};
use crate::representation::TypeId;

/// One narrowable subject. `Ord` because the environment is a
/// `BTreeMap`: deterministic iteration is a determinism invariant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NarrowingSubject {
    Local {
        name: String,
    },
    /// `$this->name` — never through `?->` (a null-safe fetch is not
    /// a stable base).
    ThisProperty {
        name: String,
    },
    /// `self::$name` / `static::$name` on the defining class.
    StaticProperty {
        name: String,
    },
}

/// The narrowing subject of one expression, seeing through
/// `Assignment` to its target so an assign-and-test condition
/// (`if (($x = f()) instanceof Foo)`) narrows the assigned subject.
pub(crate) fn subject_of(ir: &BodyIr, expression: ExpressionId) -> Option<NarrowingSubject> {
    match ir.expression(expression)? {
        BodyExpression::Variable { name } if name != "this" => {
            Some(NarrowingSubject::Local { name: name.clone() })
        }
        BodyExpression::Assignment { target, .. } => subject_of(ir, *target),
        BodyExpression::MemberAccess {
            receiver,
            member: MemberReference::Named { name },
            null_safe: false,
        } => match ir.expression(*receiver)? {
            BodyExpression::Variable {
                name: receiver_name,
            } if receiver_name == "this" => {
                Some(NarrowingSubject::ThisProperty { name: name.clone() })
            }
            _ => None,
        },
        BodyExpression::ScopedAccess {
            subject,
            member: MemberReference::Variable { name },
        } => match ir.expression(*subject)? {
            BodyExpression::NamedReference { text } => {
                let folded = text.to_ascii_lowercase();
                (folded == "self" || folded == "static")
                    .then(|| NarrowingSubject::StaticProperty { name: name.clone() })
            }
            _ => None,
        },
        _ => None,
    }
}

/// The values `===`-comparison can narrow by: exactly the forms whose
/// value set is one canonical point (or, for enum cases, one case).
pub(crate) fn is_narrowing_literal<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.is_null(db)
        || of.bool_literal_value(db).is_some()
        || of.int_literal_value(db).is_some()
        || of.float_literal_value(db).is_some()
        || of.string_literal_value(db).is_some()
        || of.enum_case_parts(db).is_some()
}

/// Is this constituent class-like enough that an unproven instanceof
/// keeps it as an intersection rather than dropping it?
fn class_like<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.class_name(db).is_some()
        || of == TypeId::object(db)
        // `intersectands` answers a singleton `vec![of]` for any
        // non-intersection type (mirroring `constituents`'s union
        // convention); a genuine intersection is always length >= 2
        // (representation.rs's invariant on `TypeData::Intersection`).
        || of.intersectands(db).len() > 1
        || of.template_bound(db).is_some()
}

/// Positive narrowing: the subject is known to be `target`.
/// Distributes over unions; a `mixed` subject becomes the target; a
/// `mixed` target narrows nothing. Per constituent: a proven subtype
/// keeps itself (precision), a proven supertype narrows to the
/// target, an undecided class-like pair intersects (two instanceofs
/// produce `Foo&Countable`), and a refuted non-class pair drops. This
/// is applied uniformly per constituent — a union member that is
/// undecided against the target intersects exactly like a bare
/// undecided subject would (dropping it instead would be unsound: a
/// non-final class not proven to implement an interface may still
/// have a subclass that does).
pub(crate) fn narrow_to<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    current: TypeId<'db>,
    target: TypeId<'db>,
) -> TypeId<'db> {
    if target.is_mixed(db) {
        return current;
    }
    if current.is_mixed(db) {
        return target;
    }
    let constituents = constituents_of(db, current);
    let narrowed = constituents.into_iter().filter_map(|constituent| {
        if subtype_of(db, files, stubs, configuration, constituent, target) == Proof::Holds {
            return Some(constituent);
        }
        if subtype_of(db, files, stubs, configuration, target, constituent) == Proof::Holds {
            return Some(target);
        }
        if class_like(db, constituent) && class_like(db, target) {
            return Some(TypeId::intersection(db, [constituent, target]));
        }
        None
    });
    TypeId::union(db, narrowed)
}

/// Negative narrowing: the subject is known not to be `target`.
/// Drops the constituents proven subtypes of the target; everything
/// undecided stays (conservative). `mixed` cannot be subtracted from.
pub(crate) fn remove_type<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    current: TypeId<'db>,
    target: TypeId<'db>,
) -> TypeId<'db> {
    if current.is_mixed(db) || target.is_mixed(db) {
        return current;
    }
    let constituents = constituents_of(db, current);
    let kept = constituents.into_iter().filter(|&constituent| {
        subtype_of(db, files, stubs, configuration, constituent, target) != Proof::Holds
    });
    TypeId::union(db, kept)
}

/// One level of union constituents (a non-union answers itself).
fn constituents_of<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> Vec<TypeId<'db>> {
    let parts = of.constituents(db);
    if parts.is_empty() { vec![of] } else { parts }
}

/// Truthiness, positive side: drop null and the literal falsy
/// scalars; a general bool tightens to `true`. Everything the rule
/// cannot decide stays — silence, never a guess.
pub(crate) fn remove_falsy<'db>(db: &'db dyn salsa::Database, current: TypeId<'db>) -> TypeId<'db> {
    let kept = constituents_of(db, current)
        .into_iter()
        .filter_map(|constituent| {
            if constituent.is_null(db) {
                return None;
            }
            if constituent == TypeId::bool(db) {
                return Some(TypeId::bool_literal(db, true));
            }
            if constituent.bool_literal_value(db) == Some(false) {
                return None;
            }
            if constituent.int_literal_value(db) == Some(0) {
                return None;
            }
            if constituent.float_literal_value(db) == Some(0.0) {
                return None;
            }
            if let Some(text) = constituent.string_literal_value(db)
                && (text.is_empty() || text == "0")
            {
                return None;
            }
            Some(constituent)
        });
    TypeId::union(db, kept)
}

/// Truthiness, negative side: keep only what can be falsy, tightened
/// to its falsy form where one exists (int to 0, bool to false).
/// `mixed` and general strings/arrays stay themselves.
pub(crate) fn keep_falsy<'db>(db: &'db dyn salsa::Database, current: TypeId<'db>) -> TypeId<'db> {
    let kept = constituents_of(db, current)
        .into_iter()
        .filter_map(|constituent| {
            if constituent.is_null(db) || constituent.is_mixed(db) {
                return Some(constituent);
            }
            if constituent == TypeId::bool(db) {
                return Some(TypeId::bool_literal(db, false));
            }
            if let Some(value) = constituent.bool_literal_value(db) {
                return (!value).then_some(constituent);
            }
            if constituent == TypeId::int(db) {
                return Some(TypeId::int_literal(db, 0));
            }
            if let Some(value) = constituent.int_literal_value(db) {
                return (value == 0).then_some(constituent);
            }
            if constituent == TypeId::float(db) {
                return Some(TypeId::float_literal(db, 0.0));
            }
            if let Some(value) = constituent.float_literal_value(db) {
                return (value == 0.0).then_some(constituent);
            }
            if constituent == TypeId::string(db) {
                return Some(TypeId::union(
                    db,
                    [
                        TypeId::string_literal(db, ""),
                        TypeId::string_literal(db, "0"),
                    ],
                ));
            }
            if let Some(text) = constituent.string_literal_value(db) {
                return (text.is_empty() || text == "0").then_some(constituent);
            }
            // Objects and known classes are always truthy; arrays and
            // everything else undecided stay (empty arrays are falsy).
            if constituent.class_name(db).is_some() || constituent == TypeId::object(db) {
                return None;
            }
            Some(constituent)
        });
    TypeId::union(db, kept)
}

/// The `is_*` family: the callee's folded global name to the type its
/// truth asserts. Unlisted names answer `None` — no facts.
pub(crate) fn type_check_target<'db>(
    db: &'db dyn salsa::Database,
    folded_callee: &str,
) -> Option<TypeId<'db>> {
    Some(match folded_callee {
        "is_string" => TypeId::string(db),
        "is_int" | "is_integer" | "is_long" => TypeId::int(db),
        "is_float" | "is_double" => TypeId::float(db),
        "is_bool" => TypeId::bool(db),
        "is_null" => TypeId::null(db),
        "is_object" => TypeId::object(db),
        "is_resource" => TypeId::resource(db),
        "is_array" => TypeId::array(
            db,
            TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
            TypeId::mixed(db),
        ),
        "is_iterable" => TypeId::iterable(db, TypeId::mixed(db), TypeId::mixed(db)),
        "is_scalar" => TypeId::union(
            db,
            [
                TypeId::int(db),
                TypeId::float(db),
                TypeId::string(db),
                TypeId::bool(db),
            ],
        ),
        "is_numeric" => TypeId::union(
            db,
            [
                TypeId::int(db),
                TypeId::float(db),
                TypeId::numeric_string(db),
            ],
        ),
        "is_countable" => TypeId::union(
            db,
            [
                TypeId::array(
                    db,
                    TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
                    TypeId::mixed(db),
                ),
                TypeId::class(db, "Countable", vec![]),
            ],
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::{AstId, BodyQuery, BodyStatement, body_ir};
    use celerrate_source::FileId;
    use celerrate_stubs::StubIndexInput;

    use super::{NarrowingSubject, subject_of};

    /// Lowers one function body and answers the IR plus the first
    /// top-level expression-statement's expression.
    fn first_expression(
        source: &str,
    ) -> (
        celerrate_semantics::BodyIr,
        celerrate_semantics::ExpressionId,
    ) {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        let body = BodyQuery::new(
            &db,
            AstId {
                file: FileId::new(0),
                index: 0,
            },
        );
        let ir = body_ir(&db, file, body).as_ref().unwrap().clone();
        let Some(BodyStatement::Expression { expression }) =
            ir.root.first().and_then(|&id| ir.statement(id)).cloned()
        else {
            panic!("expected an expression statement");
        };
        (ir, expression)
    }

    #[test]
    fn subjects_extract_from_their_stable_shapes() {
        let (ir, expression) = first_expression("<?php function f() { $x; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::Local {
                name: "x".to_owned()
            }),
        );

        let (ir, expression) = first_expression("<?php function f() { $this->prop; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::ThisProperty {
                name: "prop".to_owned()
            }),
        );

        let (ir, expression) = first_expression("<?php function f() { self::$prop; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::StaticProperty {
                name: "prop".to_owned()
            }),
        );

        // Assignment sees through to its target.
        let (ir, expression) = first_expression("<?php function f() { $x = 1; }");
        assert_eq!(
            subject_of(&ir, expression),
            Some(NarrowingSubject::Local {
                name: "x".to_owned()
            }),
        );

        // `$this` itself, `?->` fetches, and computed members are not
        // stable bases.
        let (ir, expression) = first_expression("<?php function f() { $this; }");
        assert_eq!(subject_of(&ir, expression), None);
        let (ir, expression) = first_expression("<?php function f() { $a?->prop; }");
        assert_eq!(subject_of(&ir, expression), None);
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

    #[test]
    fn narrow_to_distributes_over_unions_and_intersects_class_pairs() {
        let fixture =
            fixture(&["<?php interface Liftable {} class Foo implements Liftable {} class Bar {}"]);
        let db = &fixture.db;
        let foo = crate::TypeId::class(db, "Foo", vec![]);
        let bar = crate::TypeId::class(db, "Bar", vec![]);
        let liftable = crate::TypeId::class(db, "Liftable", vec![]);
        let null = crate::TypeId::null(db);
        let mixed = crate::TypeId::mixed(db);
        let narrow = |current, target| {
            super::narrow_to(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                current,
                target,
            )
        };
        // mixed narrows to the target outright.
        assert_eq!(narrow(mixed, foo), foo);
        // A union keeps the holding constituents and drops null.
        assert_eq!(narrow(crate::TypeId::union(db, [foo, null]), foo), foo);
        // A subtype constituent narrows to itself, not the supertype;
        // an undecided constituent (`Bar` is not proven to implement
        // `Liftable`, but is not `final` either — a subclass could)
        // intersects rather than dropping, consistent with the bare
        // `Bar` case just below: dropping it here would be unsound,
        // silently losing the Bar-subclass-that-is-Liftable case.
        assert_eq!(
            narrow(crate::TypeId::union(db, [foo, bar]), liftable),
            crate::TypeId::union(db, [foo, crate::TypeId::intersection(db, [bar, liftable])]),
        );
        // Two unrelated concrete classes cannot both hold: the
        // possibly-implementing pair intersects instead of dropping.
        assert_eq!(
            narrow(bar, liftable),
            crate::TypeId::intersection(db, [bar, liftable]),
        );
        // A scalar can never be an instance: never.
        assert_eq!(
            narrow(crate::TypeId::int(db), foo),
            crate::TypeId::never(db)
        );
        // A mixed target narrows nothing.
        assert_eq!(narrow(foo, mixed), foo);
    }

    #[test]
    fn remove_type_drops_proven_constituents_only() {
        let fixture = fixture(&["<?php class Foo {} class Bar {}"]);
        let db = &fixture.db;
        let foo = crate::TypeId::class(db, "Foo", vec![]);
        let bar = crate::TypeId::class(db, "Bar", vec![]);
        let null = crate::TypeId::null(db);
        let remove = |current, target| {
            super::remove_type(
                db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                current,
                target,
            )
        };
        assert_eq!(remove(crate::TypeId::union(db, [foo, null]), null), foo);
        assert_eq!(remove(crate::TypeId::union(db, [foo, bar]), foo), bar,);
        // Removing the whole of a non-union leaves never.
        assert_eq!(remove(foo, foo), crate::TypeId::never(db));
        // mixed cannot be subtracted from.
        assert_eq!(
            remove(crate::TypeId::mixed(db), foo),
            crate::TypeId::mixed(db)
        );
    }

    #[test]
    fn falsy_filters_split_the_scalar_families() {
        let fixture = fixture(&["<?php"]);
        let db = &fixture.db;
        let nullable_false = crate::TypeId::union(
            db,
            [
                crate::TypeId::int(db),
                crate::TypeId::bool_literal(db, false),
                crate::TypeId::null(db),
            ],
        );
        assert_eq!(
            super::remove_falsy(db, nullable_false),
            crate::TypeId::int(db),
        );
        // General bool minus false is true.
        assert_eq!(
            super::remove_falsy(db, crate::TypeId::bool(db)),
            crate::TypeId::bool_literal(db, true),
        );
        // keep_falsy is the dual: int keeps exactly 0.
        assert_eq!(
            super::keep_falsy(db, crate::TypeId::int(db)),
            crate::TypeId::int_literal(db, 0),
        );
        assert_eq!(
            super::keep_falsy(db, crate::TypeId::bool(db)),
            crate::TypeId::bool_literal(db, false),
        );
    }

    #[test]
    fn the_type_check_table_answers_the_common_family() {
        let db = TestDatabase::default();
        assert_eq!(
            super::type_check_target(&db, "is_string"),
            Some(crate::TypeId::string(&db)),
        );
        assert_eq!(
            super::type_check_target(&db, "is_int"),
            Some(crate::TypeId::int(&db)),
        );
        assert_eq!(
            super::type_check_target(&db, "is_integer"),
            Some(crate::TypeId::int(&db)),
        );
        assert_eq!(
            super::type_check_target(&db, "is_null"),
            Some(crate::TypeId::null(&db)),
        );
        assert!(super::type_check_target(&db, "is_object").is_some());
        assert!(super::type_check_target(&db, "is_numeric").is_some());
        assert!(super::type_check_target(&db, "strlen").is_none());
    }
}
