//! The array family. Every handler is a pure projection over the
//! invocation's argument types; `None` falls through to the
//! declared tier (conservative silence).

use celerrate_plugin::{TypeId, salsa};

/// `current`/`reset`/`end`: the value projection with the `false`
/// miss. Arrays and lists answer their value type; shapes union
/// their field values; anything else is `None`.
pub(crate) fn pointer_value<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let subject = arguments.first()?;
    let value = array_value_of(db, *subject)?;
    Some(TypeId::union(db, [value, TypeId::bool_literal(db, false)]))
}

/// The value type of an array-like subject, `None` when unknown.
///
/// Adjudicated resolution (tasks 6/7 defect, closed): the lattice's
/// `array_value` already answers for `TypeData::Shape` through
/// `shape_as_array` (`construction.rs`'s `array_value`/`array_key`,
/// backed by `shape_as_array`), which unions the field values
/// exactly as a hand-rolled shape projection would. A second,
/// duplicated projection over `shape_fields` here would therefore
/// be unreachable dead code, so this helper is reduced to the
/// single lattice call. An empty shape (`[]`) is not "unknown": its
/// value union is `never` (the union of zero field values), and the
/// caller's `false`-miss union collapses `never|false` to the
/// concrete `false` literal, which matches real PHP semantics for
/// `current([])`. No explicit empty-shape guard is added: the
/// natural `false` answer is the intended, correct one, not a
/// symptom worth suppressing back into `None`.
pub(crate) fn array_value_of<'db>(
    db: &'db dyn salsa::Database,
    subject: TypeId<'db>,
) -> Option<TypeId<'db>> {
    subject.array_value(db)
}

/// `array_map(callback, array, ...)`: a `null` callback with exactly
/// one array argument answers the array unchanged. Otherwise the
/// callback must carry a callable type (a `'strtoupper'` string or a
/// `mixed` callback answers `None`, leaving the declared tier's
/// answer standing); its return composes as `list<R>` for a list
/// subject (checked with `is_list` first, so a shape whose integer
/// keys are consecutive from `0` takes this branch too — `list<R>`
/// is sound and strictly more precise than a shape-key union) or for
/// the multi-array zip form (PHP reindexes when more than one array
/// is mapped), and as `array<K, R>` for any other array or shape
/// subject, keeping its key type (`array_key_of`, which unions a
/// non-list shape's key literals through the lattice).
pub(crate) fn array_map<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let callback = arguments.first()?;
    let subjects = arguments.get(1..)?;
    let first_subject = subjects.first()?;
    if callback.is_null(db) {
        return match subjects {
            [only] => Some(*only),
            _ => None,
        };
    }
    let mapped = callback.callable_return(db)?;
    if subjects.len() > 1 {
        return Some(TypeId::list(db, mapped));
    }
    if first_subject.is_list(db) {
        return Some(TypeId::list(db, mapped));
    }
    Some(TypeId::array(db, array_key_of(db, *first_subject)?, mapped))
}

/// `array_filter(array, callback?, mode?)`: a list subject (`is_list`,
/// checked first — so a shape whose integer keys are consecutive
/// from `0` counts as a list here too) loses its contiguity
/// (filtering can remove any element) but keeps non-negative integer
/// keys; any other array or shape subject keeps its key type
/// (`array_key_of`, unioning a shape's key literals). With no
/// callback, falsy constituents (`null`, `false`, `0`, `0.0`, `''`,
/// `'0'`) drop from a union value type; with a callback the value
/// type passes through unchanged (the predicate is opaque). An
/// unknown subject is `None`.
pub(crate) fn array_filter<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let subject = arguments.first()?;
    let value = array_value_of(db, *subject)?;
    let value = if arguments.len() == 1 {
        without_falsy(db, value)
    } else {
        value
    };
    let key = if subject.is_list(db) {
        TypeId::int_range(db, Some(0), None)
    } else {
        array_key_of(db, *subject)?
    };
    Some(TypeId::array(db, key, value))
}

/// The key type of an array-like subject: arrays and shapes answer
/// through the lattice's own `array_key` (the shape case already
/// unions the shape's key literals), `None` when unknown.
pub(crate) fn array_key_of<'db>(
    db: &'db dyn salsa::Database,
    subject: TypeId<'db>,
) -> Option<TypeId<'db>> {
    subject.array_key(db)
}

/// Removes the falsy constituents a bare `array_filter` discards:
/// `null`, `false`, `0`, `0.0`, `''`, `'0'`. A constituent set that
/// empties entirely stays unchanged (the conservative floor — an
/// always-falsy value is the caller's bug, not ours to `never`).
fn without_falsy<'db>(db: &'db dyn salsa::Database, value: TypeId<'db>) -> TypeId<'db> {
    let kept: Vec<TypeId<'db>> = value
        .constituents(db)
        .into_iter()
        .filter(|constituent| !is_falsy_literal(db, *constituent))
        .collect();
    if kept.is_empty() {
        value
    } else {
        TypeId::union(db, kept)
    }
}

fn is_falsy_literal<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.is_null(db)
        || of.bool_literal_value(db) == Some(false)
        || of.int_literal_value(db) == Some(0)
        || of.float_literal_value(db) == Some(0.0)
        || matches!(of.string_literal_value(db).as_deref(), Some("") | Some("0"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::{ShapeField, ShapeKey, TypeId};

    /// A two-field shape whose keys are both strings, so
    /// `shape_as_array`'s `is_list` is `false` (`ShapeKey::String`
    /// keys never collect into the `Some(Vec<i64>)` `is_list`
    /// requires) — the only way to reach the `array_key_of`
    /// (`array<K, R>`) branch rather than the `is_list` (`list<R>`)
    /// branch.
    fn shape_with_string_keys(db: &TestDatabase) -> TypeId<'_> {
        TypeId::shape(
            db,
            vec![
                ShapeField {
                    key: ShapeKey::String("a".to_owned()),
                    optional: false,
                    value: TypeId::int(db),
                },
                ShapeField {
                    key: ShapeKey::String("b".to_owned()),
                    optional: false,
                    value: TypeId::int(db),
                },
            ],
        )
    }

    #[test]
    fn array_map_with_a_null_callback_answers_the_array_unchanged() {
        let db = TestDatabase::default();
        let subject = TypeId::list(&db, TypeId::int(&db));
        let answer = super::array_map(&db, &[TypeId::null(&db), subject]).unwrap();
        assert_eq!(answer, subject);
    }

    #[test]
    fn array_map_composes_the_callable_return_over_a_list() {
        let db = TestDatabase::default();
        let callback = TypeId::callable(&db, vec![], TypeId::string(&db));
        let subject = TypeId::list(&db, TypeId::int(&db));
        assert_eq!(
            super::array_map(&db, &[callback, subject]).unwrap(),
            TypeId::list(&db, TypeId::string(&db)),
        );
    }

    #[test]
    fn array_map_keeps_the_key_type_over_an_array() {
        let db = TestDatabase::default();
        let callback = TypeId::callable(&db, vec![], TypeId::bool(&db));
        let subject = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
        assert_eq!(
            super::array_map(&db, &[callback, subject]).unwrap(),
            TypeId::array(&db, TypeId::string(&db), TypeId::bool(&db)),
        );
    }

    #[test]
    fn array_map_over_the_zip_form_answers_a_list() {
        let db = TestDatabase::default();
        let callback = TypeId::callable(&db, vec![], TypeId::int(&db));
        let first = TypeId::list(&db, TypeId::int(&db));
        let second = TypeId::list(&db, TypeId::string(&db));
        assert_eq!(
            super::array_map(&db, &[callback, first, second]).unwrap(),
            TypeId::list(&db, TypeId::int(&db)),
        );
    }

    #[test]
    fn array_map_without_a_callable_type_stays_silent() {
        let db = TestDatabase::default();
        let subject = TypeId::list(&db, TypeId::int(&db));
        assert!(super::array_map(&db, &[TypeId::string(&db), subject]).is_none());
        assert!(super::array_map(&db, &[TypeId::mixed(&db)]).is_none());
    }

    /// The non-list shape path (`array<K, R>`, `array_key_of`
    /// composed over the callable return): a shape whose keys are
    /// strings is not a list, so `array_map` must keep the shape's
    /// key union rather than answering `list<R>`.
    #[test]
    fn array_map_over_a_non_list_shape_keeps_the_key_union() {
        let db = TestDatabase::default();
        let callback = TypeId::callable(&db, vec![], TypeId::string(&db));
        let subject = shape_with_string_keys(&db);
        let key = TypeId::union(
            &db,
            [
                TypeId::string_literal(&db, "a"),
                TypeId::string_literal(&db, "b"),
            ],
        );
        assert_eq!(
            super::array_map(&db, &[callback, subject]).unwrap(),
            TypeId::array(&db, key, TypeId::string(&db)),
        );
    }

    #[test]
    fn array_filter_without_a_callback_drops_falsy_constituents() {
        let db = TestDatabase::default();
        let value = TypeId::union(
            &db,
            [
                TypeId::string(&db),
                TypeId::null(&db),
                TypeId::bool_literal(&db, false),
            ],
        );
        let subject = TypeId::array(&db, TypeId::string(&db), value);
        assert_eq!(
            super::array_filter(&db, &[subject]).unwrap(),
            TypeId::array(&db, TypeId::string(&db), TypeId::string(&db)),
        );
    }

    #[test]
    fn array_filter_over_a_list_loses_contiguity_but_keeps_int_keys() {
        let db = TestDatabase::default();
        let subject = TypeId::list(&db, TypeId::int(&db));
        assert_eq!(
            super::array_filter(&db, &[subject]).unwrap(),
            TypeId::array(&db, TypeId::int_range(&db, Some(0), None), TypeId::int(&db)),
        );
    }

    #[test]
    fn array_filter_with_a_callback_passes_the_value_through() {
        let db = TestDatabase::default();
        let value = TypeId::union(&db, [TypeId::string(&db), TypeId::null(&db)]);
        let subject = TypeId::array(&db, TypeId::string(&db), value);
        let callback = TypeId::callable(&db, vec![], TypeId::bool(&db));
        assert_eq!(
            super::array_filter(&db, &[subject, callback]).unwrap(),
            TypeId::array(&db, TypeId::string(&db), value),
        );
    }

    /// The non-list shape path's `array_filter` equivalent: a shape
    /// whose keys are strings is not a list, so `array_filter` must
    /// keep the shape's key union (`array_key_of`) rather than
    /// answering the list branch's `int_range(0, None)`.
    #[test]
    fn array_filter_over_a_non_list_shape_keeps_the_key_union() {
        let db = TestDatabase::default();
        let subject = shape_with_string_keys(&db);
        let key = TypeId::union(
            &db,
            [
                TypeId::string_literal(&db, "a"),
                TypeId::string_literal(&db, "b"),
            ],
        );
        assert_eq!(
            super::array_filter(&db, &[subject]).unwrap(),
            TypeId::array(&db, key, TypeId::int(&db)),
        );
    }
}
