//! Widening and the deterministic caps: fixpoint termination
//! depends on these being pure lattice operations, identical regardless
//! of a cycle's entry point. An arity overrun collapses
//! to the pairwise join (a common supertype, `mixed` at worst), never a
//! truncated subset, which would make the value depend on accumulation
//! order.

use crate::representation::{StringConstraint, TypeData, TypeId};

/// A union with more constituents collapses to its join.
pub const UNION_ARITY_CAP: usize = 32;

/// No canonical type nests deeper than this; construction widens the
/// children sitting at the cap to `mixed`.
pub const STRUCTURAL_DEPTH_CAP: u32 = 16;

/// Structural depth: atoms are 1, composites 1 + the deepest child.
pub(crate) fn depth_of<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> u32 {
    let children_depth = |types: &[TypeId<'db>]| {
        types
            .iter()
            .map(|child| depth_of(db, *child))
            .max()
            .unwrap_or(0)
    };
    match of.data(db) {
        TypeData::Mixed
        | TypeData::Never
        | TypeData::Void
        | TypeData::Null
        | TypeData::Object
        | TypeData::Resource
        | TypeData::Bool { .. }
        | TypeData::Int { .. }
        | TypeData::Float { .. }
        | TypeData::String { .. }
        | TypeData::EnumCase { .. }
        | TypeData::SelfPlaceholder
        | TypeData::ParentPlaceholder
        | TypeData::StaticPlaceholder => 1,
        TypeData::ClassString { argument } => {
            1 + argument.map(|child| depth_of(db, child)).unwrap_or(0)
        }
        TypeData::Union { constituents } => 1 + children_depth(constituents),
        TypeData::Intersection { intersectands } => 1 + children_depth(intersectands),
        TypeData::Array { key, value, .. } => 1 + children_depth(&[*key, *value]),
        TypeData::Shape { fields } => {
            1 + fields
                .iter()
                .map(|field| depth_of(db, field.value))
                .max()
                .unwrap_or(0)
        }
        TypeData::Class { arguments, .. } => 1 + children_depth(arguments),
        TypeData::Callable {
            parameters,
            return_type,
        } => {
            let parameter_depth = parameters
                .iter()
                .map(|parameter| depth_of(db, parameter.parameter_type))
                .max()
                .unwrap_or(0);
            1 + parameter_depth.max(depth_of(db, *return_type))
        }
        TypeData::Template { bound, .. } => 1 + depth_of(db, *bound),
        TypeData::KeyOf { subject } | TypeData::ValueOf { subject } => 1 + depth_of(db, *subject),
        TypeData::Conditional {
            subject,
            matches,
            then_branch,
            otherwise_branch,
            ..
        } => 1 + children_depth(&[*subject, *matches, *then_branch, *otherwise_branch]),
    }
}

/// A child about to enter a composite: children already at the depth
/// cap widen to `mixed`, so every constructor's result stays at the cap
/// regardless of construction order.
pub(crate) fn capped_child<'db>(db: &'db dyn salsa::Database, child: TypeId<'db>) -> TypeId<'db> {
    if depth_of(db, child) >= STRUCTURAL_DEPTH_CAP {
        TypeId::mixed(db)
    } else {
        child
    }
}

/// The deterministic pairwise join: a common supertype, hierarchy-blind
/// in this plan (unrelated classes join to `mixed`; a hierarchy-aware
/// least upper bound can refine this later without a signature change).
pub fn join<'db>(
    db: &'db dyn salsa::Database,
    left: TypeId<'db>,
    right: TypeId<'db>,
) -> TypeId<'db> {
    if left == right {
        return left;
    }
    // Unions distribute through the join.
    if let TypeData::Union { constituents } = left.data(db) {
        return constituents
            .iter()
            .fold(right, |accumulated, part| join(db, accumulated, *part));
    }
    if let TypeData::Union { constituents } = right.data(db) {
        return constituents
            .iter()
            .fold(left, |accumulated, part| join(db, accumulated, *part));
    }
    match (left.data(db), right.data(db)) {
        (TypeData::Never, _) => right,
        (_, TypeData::Never) => left,
        (TypeData::Mixed, _) | (_, TypeData::Mixed) => TypeId::mixed(db),
        (TypeData::Bool { .. }, TypeData::Bool { .. }) => TypeId::bool(db),
        (
            TypeData::Int {
                minimum: a_min,
                maximum: a_max,
            },
            TypeData::Int {
                minimum: b_min,
                maximum: b_max,
            },
        ) => {
            let minimum = match (a_min, b_min) {
                (Some(a), Some(b)) => Some(*a.min(b)),
                _ => None,
            };
            let maximum = match (a_max, b_max) {
                (Some(a), Some(b)) => Some(*a.max(b)),
                _ => None,
            };
            TypeId::int_range(db, minimum, maximum)
        }
        (TypeData::Float { .. }, TypeData::Float { .. }) => TypeId::float(db),
        (TypeData::String { .. }, TypeData::String { .. }) => TypeId::string(db),
        (
            TypeData::Array {
                key: a_key,
                value: a_value,
                is_list: a_list,
                non_empty: a_non_empty,
            },
            TypeData::Array {
                key: b_key,
                value: b_value,
                is_list: b_list,
                non_empty: b_non_empty,
            },
        ) => {
            let key = join(db, *a_key, *b_key);
            let value = join(db, *a_value, *b_value);
            match (*a_list && *b_list, *a_non_empty && *b_non_empty) {
                (true, true) => TypeId::non_empty_list(db, value),
                (true, false) => TypeId::list(db, value),
                (false, true) => TypeId::non_empty_array(db, key, value),
                (false, false) => TypeId::array(db, key, value),
            }
        }
        (TypeData::Shape { fields }, _) => {
            let (key, value, is_list, non_empty) = TypeId::shape_as_array(db, fields);
            let widened = if is_list && non_empty {
                TypeId::non_empty_list(db, value)
            } else if is_list {
                TypeId::list(db, value)
            } else if non_empty {
                TypeId::non_empty_array(db, key, value)
            } else {
                TypeId::array(db, key, value)
            };
            join(db, widened, right)
        }
        (_, TypeData::Shape { .. }) => join(db, right, left),
        (
            TypeData::Class {
                name: a_name,
                arguments: a_arguments,
            },
            TypeData::Class {
                name: b_name,
                arguments: b_arguments,
            },
        ) if a_name == b_name => {
            if a_arguments.len() == b_arguments.len() {
                let joined = a_arguments
                    .iter()
                    .zip(b_arguments.iter())
                    .map(|(a, b)| join(db, *a, *b))
                    .collect();
                TypeId::class(db, a_name, joined)
            } else {
                TypeId::class(db, a_name, vec![])
            }
        }
        (
            TypeData::EnumCase {
                enum_name: a_enum, ..
            },
            TypeData::EnumCase {
                enum_name: b_enum, ..
            },
        ) if a_enum == b_enum => TypeId::class(db, a_enum, vec![]),
        (TypeData::EnumCase { enum_name, .. }, TypeData::Class { name, arguments })
        | (TypeData::Class { name, arguments }, TypeData::EnumCase { enum_name, .. })
            if enum_name == name && arguments.is_empty() =>
        {
            TypeId::class(db, name, vec![])
        }
        _ => TypeId::mixed(db),
    }
}

/// Literal-to-general widening, recursive through unions, intersections,
/// arrays, and shape field values. Class arguments, callables, templates,
/// and the symbolic forms keep their structure (invariance; substitution
/// through those forms is `substitution.rs`'s `substitute`).
pub fn widened_literals<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> TypeId<'db> {
    match of.data(db) {
        TypeData::Bool { literal: Some(_) } => TypeId::bool(db),
        TypeData::Int {
            minimum: Some(low),
            maximum: Some(high),
        } if low == high => TypeId::int(db),
        TypeData::Float { literal: Some(_) } => TypeId::float(db),
        TypeData::String {
            constraint: StringConstraint::Literal(_),
        } => TypeId::string(db),
        TypeData::Union { constituents } => TypeId::union(
            db,
            constituents.iter().map(|part| widened_literals(db, *part)),
        ),
        TypeData::Intersection { intersectands } => TypeId::intersection(
            db,
            intersectands.iter().map(|part| widened_literals(db, *part)),
        ),
        TypeData::Array {
            key,
            value,
            is_list,
            non_empty,
        } => {
            let widened_key = widened_literals(db, *key);
            let widened_value = widened_literals(db, *value);
            match (is_list, non_empty) {
                (true, true) => TypeId::non_empty_list(db, widened_value),
                (true, false) => TypeId::list(db, widened_value),
                (false, true) => TypeId::non_empty_array(db, widened_key, widened_value),
                (false, false) => TypeId::array(db, widened_key, widened_value),
            }
        }
        TypeData::Shape { fields } => TypeId::shape(
            db,
            fields
                .iter()
                .map(|field| crate::ShapeField {
                    key: field.key.clone(),
                    optional: field.optional,
                    value: widened_literals(db, field.value),
                })
                .collect(),
        ),
        _ => of,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use super::{STRUCTURAL_DEPTH_CAP, UNION_ARITY_CAP, depth_of, join, widened_literals};
    use crate::TypeId;

    #[test]
    fn depth_counts_nesting() {
        let db = TestDatabase::default();
        assert_eq!(depth_of(&db, TypeId::int(&db)), 1);
        let array = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
        assert_eq!(depth_of(&db, array), 2);
        let nested = TypeId::array(&db, TypeId::string(&db), array);
        assert_eq!(depth_of(&db, nested), 3);
    }

    #[test]
    fn an_oversized_union_collapses_to_the_join_never_a_subset() {
        let db = TestDatabase::default();
        let literals =
            (0..(UNION_ARITY_CAP as i64 + 8)).map(|value| TypeId::int_literal(&db, value));
        let collapsed = TypeId::union(&db, literals);
        // The join of integer literals is their range hull.
        assert_eq!(
            collapsed,
            TypeId::int_range(&db, Some(0), Some(UNION_ARITY_CAP as i64 + 7))
        );
    }

    #[test]
    fn an_oversized_mixed_kind_union_collapses_to_mixed() {
        let db = TestDatabase::default();
        let mut parts: Vec<TypeId> = (0..(UNION_ARITY_CAP as i64 + 8))
            .map(|value| TypeId::int_literal(&db, value))
            .collect();
        parts.push(TypeId::class(&db, "User", vec![]));
        assert_eq!(TypeId::union(&db, parts), TypeId::mixed(&db));
    }

    #[test]
    fn depth_beyond_the_cap_widens_the_deepest_children_to_mixed() {
        let db = TestDatabase::default();
        let mut current = TypeId::int(&db);
        for _ in 0..(STRUCTURAL_DEPTH_CAP + 8) {
            current = TypeId::array(&db, TypeId::string(&db), current);
        }
        assert!(depth_of(&db, current) <= STRUCTURAL_DEPTH_CAP);
    }

    #[test]
    fn a_capped_union_constituent_triggers_mixed_absorption() {
        let db = TestDatabase::default();
        // `STRUCTURAL_DEPTH_CAP` iterations would cap the child mid-loop and
        // reset the depth (capped_child fires once depth >= the cap); one
        // fewer iteration is the true reachable boundary: depth exactly at
        // `STRUCTURAL_DEPTH_CAP`, uncapped, and constructible.
        let mut deep = TypeId::int(&db);
        for _ in 0..(STRUCTURAL_DEPTH_CAP - 1) {
            deep = TypeId::array(&db, TypeId::string(&db), deep);
        }
        assert_eq!(depth_of(&db, deep), STRUCTURAL_DEPTH_CAP);
        assert_eq!(
            TypeId::union(&db, [deep, TypeId::string(&db)]),
            TypeId::mixed(&db)
        );
    }

    #[test]
    fn a_capped_intersection_constituent_disappears() {
        let db = TestDatabase::default();
        // See the comment above: one fewer than `STRUCTURAL_DEPTH_CAP`
        // iterations is the reachable boundary for a depth exactly at the cap.
        let mut deep = TypeId::int(&db);
        for _ in 0..(STRUCTURAL_DEPTH_CAP - 1) {
            deep = TypeId::array(&db, TypeId::string(&db), deep);
        }
        assert_eq!(depth_of(&db, deep), STRUCTURAL_DEPTH_CAP);
        assert_eq!(
            TypeId::intersection(&db, [deep, TypeId::string(&db)]),
            TypeId::string(&db)
        );
    }

    #[test]
    fn the_cap_applies_to_every_growing_constructor() {
        let db = TestDatabase::default();
        let mut current = TypeId::int(&db);
        for _ in 0..(STRUCTURAL_DEPTH_CAP + 8) {
            current = TypeId::class(&db, "Collection", vec![current]);
        }
        assert!(depth_of(&db, current) <= STRUCTURAL_DEPTH_CAP);
    }

    #[test]
    fn join_is_the_deterministic_common_supertype() {
        let db = TestDatabase::default();
        assert_eq!(
            join(&db, TypeId::int(&db), TypeId::int(&db)),
            TypeId::int(&db)
        );
        assert_eq!(
            join(
                &db,
                TypeId::int_literal(&db, 1),
                TypeId::int_literal(&db, 5)
            ),
            TypeId::int_range(&db, Some(1), Some(5))
        );
        assert_eq!(
            join(&db, TypeId::bool_literal(&db, true), TypeId::bool(&db)),
            TypeId::bool(&db)
        );
        assert_eq!(
            join(
                &db,
                TypeId::string_literal(&db, "a"),
                TypeId::non_empty_string(&db)
            ),
            TypeId::string(&db)
        );
        assert_eq!(
            join(&db, TypeId::never(&db), TypeId::int(&db)),
            TypeId::int(&db)
        );
        assert_eq!(
            join(&db, TypeId::null(&db), TypeId::int(&db)),
            TypeId::mixed(&db)
        );
        // Arrays join structurally: the list flag drops (only one side is
        // a list), keys join through the hierarchy-blind rule (int and
        // string join to mixed), values take the range hull.
        let of_int = TypeId::list(&db, TypeId::int_literal(&db, 1));
        let of_string = TypeId::array(&db, TypeId::string(&db), TypeId::int_literal(&db, 2));
        assert_eq!(
            join(&db, of_int, of_string),
            TypeId::array(
                &db,
                TypeId::mixed(&db),
                TypeId::int_range(&db, Some(1), Some(2)),
            )
        );
        // Same-name classes join argumentwise; unrelated classes join to mixed.
        assert_eq!(
            join(
                &db,
                TypeId::class(&db, "Collection", vec![TypeId::int_literal(&db, 1)]),
                TypeId::class(&db, "Collection", vec![TypeId::int_literal(&db, 2)]),
            ),
            TypeId::class(
                &db,
                "Collection",
                vec![TypeId::int_range(&db, Some(1), Some(2))]
            )
        );
        assert_eq!(
            join(
                &db,
                TypeId::class(&db, "User", vec![]),
                TypeId::class(&db, "Order", vec![])
            ),
            TypeId::mixed(&db)
        );
    }

    #[test]
    fn widening_generalizes_literals_recursively() {
        let db = TestDatabase::default();
        assert_eq!(
            widened_literals(&db, TypeId::int_literal(&db, 42)),
            TypeId::int(&db)
        );
        assert_eq!(
            widened_literals(&db, TypeId::string_literal(&db, "active")),
            TypeId::string(&db)
        );
        assert_eq!(
            widened_literals(&db, TypeId::bool_literal(&db, true)),
            TypeId::bool(&db)
        );
        assert_eq!(
            widened_literals(&db, TypeId::float_literal(&db, 1.5)),
            TypeId::float(&db)
        );
        // Bounded ranges are not literals and stay.
        let range = TypeId::int_range(&db, Some(1), None);
        assert_eq!(widened_literals(&db, range), range);
        // Recursion into unions and arrays; class arguments stay (invariance).
        let nullable_literal = TypeId::union(&db, [TypeId::int_literal(&db, 1), TypeId::null(&db)]);
        assert_eq!(
            widened_literals(&db, nullable_literal),
            TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)])
        );
        let list = TypeId::list(&db, TypeId::int_literal(&db, 1));
        assert_eq!(
            widened_literals(&db, list),
            TypeId::list(&db, TypeId::int(&db))
        );
        let generic = TypeId::class(&db, "Collection", vec![TypeId::int_literal(&db, 1)]);
        assert_eq!(widened_literals(&db, generic), generic);
    }
}
