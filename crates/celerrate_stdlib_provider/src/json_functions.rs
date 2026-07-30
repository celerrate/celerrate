//! The JSON family. Every handler is a pure projection over the
//! invocation's argument types; `None` falls through to the
//! declared tier (conservative silence).

use celerrate_plugin::{TypeContext, TypeId};

/// PHP's decode-side flag selecting the array branch.
pub(crate) const JSON_OBJECT_AS_ARRAY: i64 = 1;

/// `json_decode(json, associative?, depth?, flags?)`. The scalar
/// tail (`bool|float|int|string|null`) is always present. PHP's
/// `ext/json/json.c` overrides the
/// `JSON_OBJECT_AS_ARRAY` flag with a non-`null` `$associative` in
/// BOTH directions ("for BC reasons"): a `true` associative literal
/// selects the array branch and a `false` associative literal selects
/// the object branch, regardless of the flags argument. The flags
/// argument decides only when `associative` is the `null` literal
/// (the `?bool $associative = null` default since PHP 7.4) or absent:
/// an integer-literal flags argument with the `JSON_OBJECT_AS_ARRAY`
/// bit set selects the array branch, without it selects the object
/// branch, and a non-literal flags argument leaves the answer
/// undecided (both branches). Any other associative reading (present,
/// neither a bool literal nor `null`) also answers both branches,
/// regardless of flags — it may be `false` at runtime.
pub(crate) fn json_decode<'db>(
    context: TypeContext<'db>,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    if arguments.is_empty() {
        return None;
    }
    let flags = arguments.get(3);
    let flag_selects_array = flags
        .and_then(|flags| context.int_literal_value(*flags))
        .is_some_and(|value| value & JSON_OBJECT_AS_ARRAY != 0);
    let flags_undecided = flags.is_some_and(|flags| context.int_literal_value(*flags).is_none());
    Some(match arguments.get(1) {
        None => flags_branch(context, flag_selects_array, flags_undecided),
        Some(associative) => match context.bool_literal_value(*associative) {
            // A non-`null` associative overrides the flag in both
            // directions (PHP's BC-reasons override), regardless of
            // what the flags argument says.
            Some(true) => array_branch(context),
            Some(false) => object_branch(context),
            // An explicit `null` behaves exactly like an absent
            // argument (`?bool $associative = null` since PHP 7.4):
            // the flags argument decides.
            None if context.is_null(*associative) => {
                flags_branch(context, flag_selects_array, flags_undecided)
            }
            _ => both_branches(context),
        },
    })
}

/// The answer when `associative` is `null` or absent: the flags
/// argument is the sole decider.
fn flags_branch<'db>(
    context: TypeContext<'db>,
    flag_selects_array: bool,
    flags_undecided: bool,
) -> TypeId<'db> {
    if flags_undecided {
        both_branches(context)
    } else if flag_selects_array {
        array_branch(context)
    } else {
        object_branch(context)
    }
}

fn scalar_tail<'db>(context: TypeContext<'db>) -> [TypeId<'db>; 5] {
    [
        context.bool(),
        context.float(),
        context.int(),
        context.string(),
        context.null(),
    ]
}

pub(crate) fn array_branch<'db>(context: TypeContext<'db>) -> TypeId<'db> {
    let array = context.array(
        context.union([context.int(), context.string()]),
        context.mixed(),
    );
    context.union(scalar_tail(context).into_iter().chain([array]))
}

pub(crate) fn object_branch<'db>(context: TypeContext<'db>) -> TypeId<'db> {
    let object = context.class("stdclass", vec![]);
    context.union(scalar_tail(context).into_iter().chain([object]))
}

pub(crate) fn both_branches<'db>(context: TypeContext<'db>) -> TypeId<'db> {
    context.union([array_branch(context), object_branch(context)])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::TypeId;
    use celerrate_types::testing_type_context;

    #[test]
    fn json_decode_defaults_to_the_object_branch() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let answer = super::json_decode(context, &[TypeId::string(&db)]).unwrap();
        assert_eq!(answer, super::object_branch(context));
    }

    // Real PHP 8.5.0 truth table (empirically verified) for the four
    // combinations of a literal/`null` `associative` crossed with a
    // flags argument that does or does not carry
    // `JSON_OBJECT_AS_ARRAY`. A non-`null` `associative` always wins;
    // the flag only decides when `associative` is `null` or absent.

    #[test]
    fn an_associative_true_literal_selects_the_array_branch_even_when_the_flag_is_unset() {
        // Truth table row: `json_decode($s, true, 512, 0)` => array.
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let answer = super::json_decode(
            context,
            &[
                TypeId::string(&db),
                TypeId::bool_literal(&db, true),
                TypeId::int_literal(&db, 512),
                TypeId::int_literal(&db, 0),
            ],
        )
        .unwrap();
        assert_eq!(answer, super::array_branch(context));
    }

    #[test]
    fn an_associative_false_literal_overrides_the_object_as_array_flag() {
        // Truth table row: `json_decode($s, false, 512,
        // JSON_OBJECT_AS_ARRAY)` => stdClass. `ext/json/json.c`
        // carries this override as an explicit "for BC reasons"
        // comment: a non-null `$associative` beats the flag in both
        // directions, so the flag being set does not win here.
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let answer = super::json_decode(
            context,
            &[
                TypeId::string(&db),
                TypeId::bool_literal(&db, false),
                TypeId::int_literal(&db, 512),
                TypeId::int_literal(&db, super::JSON_OBJECT_AS_ARRAY),
            ],
        )
        .unwrap();
        assert_eq!(answer, super::object_branch(context));
    }

    #[test]
    fn a_null_associative_falls_back_to_the_object_as_array_flag_when_set() {
        // Truth table row: `json_decode($s, null, 512,
        // JSON_OBJECT_AS_ARRAY)` => array. `associative` is `null`,
        // so (unlike the two tests above) the flag is the decider.
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let answer = super::json_decode(
            context,
            &[
                TypeId::string(&db),
                TypeId::null(&db),
                TypeId::int_literal(&db, 512),
                TypeId::int_literal(&db, super::JSON_OBJECT_AS_ARRAY),
            ],
        )
        .unwrap();
        assert_eq!(answer, super::array_branch(context));
    }

    #[test]
    fn a_null_associative_falls_back_to_the_object_as_array_flag_when_unset() {
        // Truth table row: `json_decode($s, null, 512, 0)` =>
        // stdClass. Also pins `flags_undecided`: mutating it to
        // `flags.is_some()` would treat this decided, bit-unset flags
        // argument as undecided and answer `both_branches` instead.
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let answer = super::json_decode(
            context,
            &[
                TypeId::string(&db),
                TypeId::null(&db),
                TypeId::int_literal(&db, 512),
                TypeId::int_literal(&db, 512),
            ],
        )
        .unwrap();
        assert_eq!(answer, super::object_branch(context));
    }

    #[test]
    fn an_undecided_associative_argument_answers_both_branches() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let answer =
            super::json_decode(context, &[TypeId::string(&db), TypeId::bool(&db)]).unwrap();
        assert_eq!(answer, super::both_branches(context));
    }

    #[test]
    fn an_explicit_null_associative_behaves_like_an_absent_one() {
        // `?bool $associative = null` since PHP 7.4: an explicit `null`
        // is exactly the absent argument.
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let answer =
            super::json_decode(context, &[TypeId::string(&db), TypeId::null(&db)]).unwrap();
        assert_eq!(answer, super::object_branch(context));
    }

    // The `json_decode` tests above pin dispatch by comparing
    // against `super::object_branch`/`array_branch`/`both_branches` —
    // the same constructors the handler calls. That pins WHICH branch
    // fires but is tautological about what the branches actually
    // contain: deleting `null` from `scalar_tail`, or misspelling
    // `"stdclass"`, would leave all of them green. These two tests
    // pin the branches' actual content directly instead.

    #[test]
    fn the_object_branch_carries_stdclass_and_null() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let constituents = super::object_branch(context).constituents(&db);
        assert!(
            constituents.contains(&TypeId::class(&db, "stdclass", vec![])),
            "the object branch must carry `stdClass`, got {constituents:?}",
        );
        assert!(
            constituents.contains(&TypeId::null(&db)),
            "`null` must stay in the object branch, got {constituents:?}",
        );
    }

    #[test]
    fn the_array_branch_carries_the_array_type_and_null() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        let constituents = super::array_branch(context).constituents(&db);
        let array = TypeId::array(
            &db,
            TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]),
            TypeId::mixed(&db),
        );
        assert!(
            constituents.contains(&array),
            "the array branch must carry `array<array-key, mixed>`, got {constituents:?}",
        );
        assert!(
            constituents.contains(&TypeId::null(&db)),
            "`null` must stay in the array branch, got {constituents:?}",
        );
    }
}
