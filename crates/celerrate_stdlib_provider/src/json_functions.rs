//! The JSON family. Every handler is a pure projection over the
//! invocation's argument types; `None` falls through to the
//! declared tier (conservative silence).

use celerrate_plugin::{TypeId, salsa};

/// PHP's decode-side flag selecting the array branch.
pub(crate) const JSON_OBJECT_AS_ARRAY: i64 = 1;

/// `json_decode(json, associative?, depth?, flags?)`: decision 12.
/// The scalar tail (`bool|float|int|string|null`) is always present.
/// A `true` associative literal, or a `JSON_OBJECT_AS_ARRAY` bit set
/// on an integer-literal flags argument, selects the array branch
/// outright — the flags check wins over any other reading of
/// `associative`. Absent, `null` (the `?bool $associative = null`
/// default since PHP 7.4), or a `false` literal select the object
/// branch, unless the flags argument is present but not an integer
/// literal (the array flag might or might not be set, so the answer
/// stays undecided). Any other associative reading (present, neither
/// a bool literal nor `null`) answers both branches.
pub(crate) fn json_decode<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    if arguments.is_empty() {
        return None;
    }
    let flags = arguments.get(3);
    let flag_selects_array = flags
        .and_then(|flags| flags.int_literal_value(db))
        .is_some_and(|value| value & JSON_OBJECT_AS_ARRAY != 0);
    let flags_undecided = flags.is_some_and(|flags| flags.int_literal_value(db).is_none());
    Some(match arguments.get(1) {
        _ if flag_selects_array => array_branch(db),
        None => {
            if flags_undecided {
                both_branches(db)
            } else {
                object_branch(db)
            }
        }
        Some(associative) => match associative.bool_literal_value(db) {
            Some(true) => array_branch(db),
            Some(false) if !flags_undecided => object_branch(db),
            // An explicit `null` behaves exactly like an absent
            // argument (`?bool $associative = null` since PHP 7.4).
            None if associative.is_null(db) && !flags_undecided => object_branch(db),
            _ => both_branches(db),
        },
    })
}

fn scalar_tail<'db>(db: &'db dyn salsa::Database) -> [TypeId<'db>; 5] {
    [
        TypeId::bool(db),
        TypeId::float(db),
        TypeId::int(db),
        TypeId::string(db),
        TypeId::null(db),
    ]
}

pub(crate) fn array_branch<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    let array = TypeId::array(
        db,
        TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
        TypeId::mixed(db),
    );
    TypeId::union(db, scalar_tail(db).into_iter().chain([array]))
}

pub(crate) fn object_branch<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    let object = TypeId::class(db, "stdclass", vec![]);
    TypeId::union(db, scalar_tail(db).into_iter().chain([object]))
}

pub(crate) fn both_branches<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    TypeId::union(db, [array_branch(db), object_branch(db)])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::TypeId;

    #[test]
    fn json_decode_defaults_to_the_object_branch() {
        let db = TestDatabase::default();
        let answer = super::json_decode(&db, &[TypeId::string(&db)]).unwrap();
        assert_eq!(answer, super::object_branch(&db));
    }

    #[test]
    fn an_associative_true_literal_selects_the_array_branch() {
        let db = TestDatabase::default();
        let answer =
            super::json_decode(&db, &[TypeId::string(&db), TypeId::bool_literal(&db, true)])
                .unwrap();
        assert_eq!(answer, super::array_branch(&db));
    }

    #[test]
    fn the_object_as_array_flag_selects_the_array_branch() {
        let db = TestDatabase::default();
        let answer = super::json_decode(
            &db,
            &[
                TypeId::string(&db),
                TypeId::bool_literal(&db, false),
                TypeId::int_literal(&db, 512),
                TypeId::int_literal(&db, super::JSON_OBJECT_AS_ARRAY),
            ],
        )
        .unwrap();
        assert_eq!(answer, super::array_branch(&db));
    }

    #[test]
    fn an_undecided_associative_argument_answers_both_branches() {
        let db = TestDatabase::default();
        let answer = super::json_decode(&db, &[TypeId::string(&db), TypeId::bool(&db)]).unwrap();
        assert_eq!(answer, super::both_branches(&db));
    }

    #[test]
    fn an_explicit_null_associative_behaves_like_an_absent_one() {
        // `?bool $associative = null` since PHP 7.4: an explicit `null`
        // is exactly the absent argument (decision 12).
        let db = TestDatabase::default();
        let answer = super::json_decode(&db, &[TypeId::string(&db), TypeId::null(&db)]).unwrap();
        assert_eq!(answer, super::object_branch(&db));
    }
}
