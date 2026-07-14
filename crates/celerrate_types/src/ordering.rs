//! The canonical order of the lattice: structural (by rank, name, and
//! shape), never by interner handle. Interning order is timing-dependent
//! under parallel fan-out, so a handle-based sort would make canonical
//! forms nondeterministic and break the byte-identical harness.

use std::cmp::Ordering;

use crate::representation::{StringConstraint, TypeData, TypeId};

/// The fixed rank of each variant. Extending tasks append new variants
/// at their documented rank; existing ranks never change.
fn rank(data: &TypeData<'_>) -> u8 {
    match data {
        TypeData::Never => 0,
        TypeData::Void => 1,
        TypeData::Null => 2,
        TypeData::Bool { .. } => 3,
        TypeData::Int { .. } => 4,
        TypeData::Float { .. } => 5,
        TypeData::String { .. } => 6,
        TypeData::Intersection { .. } => 22,
        TypeData::Union { .. } => 23,
        TypeData::Mixed => 24,
        TypeData::Object => 10,
        TypeData::Resource => 11,
        TypeData::Array { .. } => 8,
        TypeData::Shape { .. } => 9,
    }
}

fn order_string_constraint(left: &StringConstraint, right: &StringConstraint) -> Ordering {
    fn constraint_rank(constraint: &StringConstraint) -> u8 {
        match constraint {
            StringConstraint::General => 0,
            StringConstraint::NonEmpty => 1,
            StringConstraint::Numeric => 2,
            StringConstraint::LiteralMarker => 3,
            StringConstraint::Literal(_) => 4,
        }
    }
    constraint_rank(left)
        .cmp(&constraint_rank(right))
        .then_with(|| match (left, right) {
            (StringConstraint::Literal(a), StringConstraint::Literal(b)) => a.cmp(b),
            _ => Ordering::Equal,
        })
}

pub(crate) fn order_types<'db>(
    db: &'db dyn salsa::Database,
    left: &[TypeId<'db>],
    right: &[TypeId<'db>],
) -> Ordering {
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = structural_order(db, *a, *b);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn order_shape_fields<'db>(
    db: &'db dyn salsa::Database,
    left: &[crate::representation::ShapeField<'db>],
    right: &[crate::representation::ShapeField<'db>],
) -> Ordering {
    for (left_field, right_field) in left.iter().zip(right.iter()) {
        let ordering = left_field
            .key
            .cmp(&right_field.key)
            .then(left_field.optional.cmp(&right_field.optional))
            .then_with(|| structural_order(db, left_field.value, right_field.value));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

/// The total, deterministic, structural order over interned types.
pub(crate) fn structural_order<'db>(
    db: &'db dyn salsa::Database,
    left: TypeId<'db>,
    right: TypeId<'db>,
) -> Ordering {
    if left == right {
        return Ordering::Equal;
    }
    let left_data = left.data(db);
    let right_data = right.data(db);
    rank(left_data)
        .cmp(&rank(right_data))
        .then_with(|| match (left_data, right_data) {
            (TypeData::Bool { literal: a }, TypeData::Bool { literal: b }) => a.cmp(b),
            (
                TypeData::Int {
                    minimum: a_min,
                    maximum: a_max,
                },
                TypeData::Int {
                    minimum: b_min,
                    maximum: b_max,
                },
            ) => (a_min, a_max).cmp(&(b_min, b_max)),
            (TypeData::Float { literal: a }, TypeData::Float { literal: b }) => a.cmp(b),
            (TypeData::String { constraint: a }, TypeData::String { constraint: b }) => {
                order_string_constraint(a, b)
            }
            (TypeData::Union { constituents: a }, TypeData::Union { constituents: b })
            | (
                TypeData::Intersection { intersectands: a },
                TypeData::Intersection { intersectands: b },
            ) => order_types(db, a, b),
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
            ) => (a_list, a_non_empty)
                .cmp(&(b_list, b_non_empty))
                .then_with(|| structural_order(db, *a_key, *b_key))
                .then_with(|| structural_order(db, *a_value, *b_value)),
            (TypeData::Shape { fields: a }, TypeData::Shape { fields: b }) => {
                order_shape_fields(db, a, b)
            }
            // Same rank with no fields is equal; interning made left == right
            // impossible here, so this arm is unreachable for atoms but kept
            // total for safety.
            _ => Ordering::Equal,
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::cmp::Ordering;

    use celerrate_db::testing::TestDatabase;

    use super::structural_order;
    use crate::TypeId;

    #[test]
    fn the_order_is_total_deterministic_and_structural() {
        let db = TestDatabase::default();
        let null = TypeId::null(&db);
        let int = TypeId::int(&db);
        let string = TypeId::string(&db);
        // Rank order: Null(2) < Int(4) < String(6).
        assert_eq!(structural_order(&db, null, int), Ordering::Less);
        assert_eq!(structural_order(&db, int, string), Ordering::Less);
        assert_eq!(structural_order(&db, string, string), Ordering::Equal);
        // Same rank compares fields: bounded ranges order by bounds, None first.
        let low = TypeId::int_range(&db, Some(1), Some(3));
        let unbounded = TypeId::int(&db);
        assert_eq!(structural_order(&db, unbounded, low), Ordering::Less);
        // String literals order by value.
        let a = TypeId::string_literal(&db, "a");
        let b = TypeId::string_literal(&db, "b");
        assert_eq!(structural_order(&db, a, b), Ordering::Less);
    }

    #[test]
    fn equal_order_means_equal_handle() {
        let db = TestDatabase::default();
        let left = TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)]);
        let right = TypeId::union(&db, [TypeId::null(&db), TypeId::int(&db)]);
        assert_eq!(structural_order(&db, left, right), Ordering::Equal);
        assert_eq!(left, right);
    }
}
