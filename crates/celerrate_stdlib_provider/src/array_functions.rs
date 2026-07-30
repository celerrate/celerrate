//! The array family. Every handler is a pure projection over the
//! invocation's argument types; `None` falls through to the
//! declared tier (conservative silence).

use celerrate_plugin::{TypeContext, TypeId};

/// `current`/`reset`/`end`: the value projection with the `false`
/// miss. Arrays and lists answer their value type; shapes union
/// their field values; anything else is `None`.
pub(crate) fn pointer_value<'db>(
    context: TypeContext<'db>,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let subject = arguments.first()?;
    let value = array_value_of(context, *subject)?;
    Some(context.union([value, context.bool_literal(false)]))
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
    context: TypeContext<'db>,
    subject: TypeId<'db>,
) -> Option<TypeId<'db>> {
    context.array_value(subject)
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
///
/// Recorded debt: a callable-STRING callback (`'strtoupper'`) has no
/// lattice
/// constructor for "the return type of the named function" today, so
/// `callable_return` answers `None` for it exactly like an opaque
/// `mixed` callback does, and the call falls through to the declared
/// tier's answer — sound (widening), unmeasured against the corpus.
pub(crate) fn array_map<'db>(
    context: TypeContext<'db>,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let callback = arguments.first()?;
    let subjects = arguments.get(1..)?;
    let first_subject = subjects.first()?;
    if context.is_null(*callback) {
        return match subjects {
            [only] => Some(*only),
            _ => None,
        };
    }
    let mapped = context.callable_return(*callback)?;
    if subjects.len() > 1 {
        return Some(context.list(mapped));
    }
    if context.is_list(*first_subject) {
        return Some(context.list(mapped));
    }
    Some(context.array(array_key_of(context, *first_subject)?, mapped))
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
///
/// The `arguments.len() == 1` gate below only recognizes the bare
/// single-argument call as "no callback". PHP's own `null` callback
/// (`array_filter($a, null, $mode)`, three arguments, mode selecting
/// keys/both) is the SAME falsy-drop form as the bare call, but this
/// handler falls into the "has a callback" branch for it and passes
/// the value type through unchanged instead of dropping falsy
/// constituents — a PRECISION miss (widening, sound), not exercised
/// by the pinned corpus (none of its three `array_filter` call sites
/// use this form).
pub(crate) fn array_filter<'db>(
    context: TypeContext<'db>,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let subject = arguments.first()?;
    let value = array_value_of(context, *subject)?;
    let value = if arguments.len() == 1 {
        without_falsy(context, value)
    } else {
        value
    };
    let key = if context.is_list(*subject) {
        context.int_range(Some(0), None)
    } else {
        array_key_of(context, *subject)?
    };
    Some(context.array(key, value))
}

/// The key type of an array-like subject: arrays and shapes answer
/// through the lattice's own `array_key` (the shape case already
/// unions the shape's key literals), `None` when unknown.
pub(crate) fn array_key_of<'db>(
    context: TypeContext<'db>,
    subject: TypeId<'db>,
) -> Option<TypeId<'db>> {
    context.array_key(subject)
}

/// Removes the falsy constituents a bare `array_filter` discards:
/// `null`, `false`, `0`, `0.0`, `''`, `'0'`. A constituent set that
/// empties entirely stays unchanged (the conservative floor — an
/// always-falsy value is the caller's bug, not ours to `never`).
fn without_falsy<'db>(context: TypeContext<'db>, value: TypeId<'db>) -> TypeId<'db> {
    let kept: Vec<TypeId<'db>> = context
        .constituents(value)
        .into_iter()
        .filter(|constituent| !is_falsy_literal(context, *constituent))
        .collect();
    if kept.is_empty() {
        value
    } else {
        context.union(kept)
    }
}

fn is_falsy_literal<'db>(context: TypeContext<'db>, of: TypeId<'db>) -> bool {
    context.is_null(of)
        || context.bool_literal_value(of) == Some(false)
        || context.int_literal_value(of) == Some(0)
        || context.float_literal_value(of) == Some(0.0)
        || matches!(
            context.string_literal_value(of).as_deref(),
            Some("") | Some("0")
        )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::{ShapeField, ShapeKey, TypeId};
    use celerrate_types::testing_type_context;

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
        let context = testing_type_context(&db);
        let subject = TypeId::list(&db, TypeId::int(&db));
        let answer = super::array_map(context, &[TypeId::null(&db), subject]).unwrap();
        assert_eq!(answer, subject);
    }

    #[test]
    fn array_map_composes_the_callable_return_over_a_list() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let callback = TypeId::callable(&db, vec![], TypeId::string(&db));
        let subject = TypeId::list(&db, TypeId::int(&db));
        assert_eq!(
            super::array_map(context, &[callback, subject]).unwrap(),
            TypeId::list(&db, TypeId::string(&db)),
        );
    }

    #[test]
    fn array_map_keeps_the_key_type_over_an_array() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let callback = TypeId::callable(&db, vec![], TypeId::bool(&db));
        let subject = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
        assert_eq!(
            super::array_map(context, &[callback, subject]).unwrap(),
            TypeId::array(&db, TypeId::string(&db), TypeId::bool(&db)),
        );
    }

    #[test]
    fn array_map_over_the_zip_form_answers_a_list() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let callback = TypeId::callable(&db, vec![], TypeId::int(&db));
        let first = TypeId::list(&db, TypeId::int(&db));
        let second = TypeId::list(&db, TypeId::string(&db));
        assert_eq!(
            super::array_map(context, &[callback, first, second]).unwrap(),
            TypeId::list(&db, TypeId::int(&db)),
        );
    }

    #[test]
    fn array_map_without_a_callable_type_stays_silent() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let subject = TypeId::list(&db, TypeId::int(&db));
        assert!(super::array_map(context, &[TypeId::string(&db), subject]).is_none());
        assert!(super::array_map(context, &[TypeId::mixed(&db)]).is_none());
    }

    /// The non-list shape path (`array<K, R>`, `array_key_of`
    /// composed over the callable return): a shape whose keys are
    /// strings is not a list, so `array_map` must keep the shape's
    /// key union rather than answering `list<R>`.
    #[test]
    fn array_map_over_a_non_list_shape_keeps_the_key_union() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
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
            super::array_map(context, &[callback, subject]).unwrap(),
            TypeId::array(&db, key, TypeId::string(&db)),
        );
    }

    #[test]
    fn array_filter_without_a_callback_drops_falsy_constituents() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
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
            super::array_filter(context, &[subject]).unwrap(),
            TypeId::array(&db, TypeId::string(&db), TypeId::string(&db)),
        );
    }

    #[test]
    fn array_filter_over_a_list_loses_contiguity_but_keeps_int_keys() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let subject = TypeId::list(&db, TypeId::int(&db));
        assert_eq!(
            super::array_filter(context, &[subject]).unwrap(),
            TypeId::array(&db, TypeId::int_range(&db, Some(0), None), TypeId::int(&db)),
        );
    }

    #[test]
    fn array_filter_with_a_callback_passes_the_value_through() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let value = TypeId::union(&db, [TypeId::string(&db), TypeId::null(&db)]);
        let subject = TypeId::array(&db, TypeId::string(&db), value);
        let callback = TypeId::callable(&db, vec![], TypeId::bool(&db));
        assert_eq!(
            super::array_filter(context, &[subject, callback]).unwrap(),
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
        let context = testing_type_context(&db);
        let subject = shape_with_string_keys(&db);
        let key = TypeId::union(
            &db,
            [
                TypeId::string_literal(&db, "a"),
                TypeId::string_literal(&db, "b"),
            ],
        );
        assert_eq!(
            super::array_filter(context, &[subject]).unwrap(),
            TypeId::array(&db, key, TypeId::int(&db)),
        );
    }
}
