//! The lattice representation. `TypeData` stays inside this private
//! module: the public surface is the interned [`TypeId`] handle plus
//! constructors and query methods, never a matchable enum.

/// One parameter of a callable signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct CallableParameter<'db> {
    pub parameter_type: TypeId<'db>,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}

/// One array-shape key. `Integer` sorts before `String`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShapeKey {
    Integer(i64),
    String(String),
}

/// One field of an array shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct ShapeField<'db> {
    pub key: ShapeKey,
    pub optional: bool,
    pub value: TypeId<'db>,
}

/// A float literal by bit pattern, so literals are `Eq`/`Hash`-safe.
/// Every NaN canonicalizes to one pattern; `0.0` and `-0.0` stay
/// distinct interned literals (their join is `float`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FloatBits(u64);

impl FloatBits {
    pub fn from_value(value: f64) -> Self {
        if value.is_nan() {
            return Self(f64::NAN.to_bits());
        }
        Self(value.to_bits())
    }

    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// The string subtypes the PHPStan dialect carries (spec section 3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StringConstraint {
    General,
    NonEmpty,
    Numeric,
    LiteralMarker,
    Literal(String),
}

/// The lattice. NEVER derive `Ord`/`PartialOrd` here: child handles
/// would compare by interner id, which is timing-dependent under
/// parallel fan-out; `ordering::structural_order` owns comparison.
///
/// `salsa::Update` is derived (rather than hand-implemented) solely
/// because the `Union`/`Intersection` variants are self-referential
/// (`TypeId<'db>` inside `TypeData<'db>`, interned by the same struct);
/// salsa's interned-struct macro requires it to well-form the `'db`
/// lifetime in that recursive case.
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum TypeData<'db> {
    Mixed,
    Never,
    Void,
    Null,
    Object,
    Resource,
    Bool {
        literal: Option<bool>,
    },
    Int {
        minimum: Option<i64>,
        maximum: Option<i64>,
    },
    Float {
        literal: Option<FloatBits>,
    },
    String {
        constraint: StringConstraint,
    },
    /// Flattened, deduplicated, structurally sorted, length >= 2.
    Union {
        constituents: Vec<TypeId<'db>>,
    },
    /// Flattened, deduplicated, structurally sorted, length >= 2.
    Intersection {
        intersectands: Vec<TypeId<'db>>,
    },
    /// `array<K, V>` and its list and non-empty refinements. A list
    /// always stores the general `int` key.
    Array {
        key: TypeId<'db>,
        value: TypeId<'db>,
        is_list: bool,
        non_empty: bool,
    },
    /// A sealed array shape, fields sorted by key.
    Shape {
        fields: Vec<ShapeField<'db>>,
    },
    /// `class-string` / `class-string<T>`: the primary template binder,
    /// never lowered to `string` (spec section 3). Rank 7.
    ClassString {
        argument: Option<TypeId<'db>>,
    },
    /// A class, interface, trait, or enum type, name pre-folded,
    /// carrying its generic arguments. Rank 12.
    Class {
        name: String,
        arguments: Vec<TypeId<'db>>,
    },
    /// One enum case: enum key folded, case name verbatim
    /// (case-sensitive, matching the member boundary). Rank 13.
    EnumCase {
        enum_name: String,
        case_name: String,
    },
    /// A callable signature. Rank 14.
    Callable {
        parameters: Vec<CallableParameter<'db>>,
        return_type: TypeId<'db>,
    },
    /// A template variable: a lattice citizen before any call-site
    /// substitution. The scope string discriminates same-named
    /// templates of different declarations. Rank 15.
    Template {
        scope: String,
        name: String,
        bound: TypeId<'db>,
    },
    /// Symbolic `key-of<T>` (decidable subjects evaluated at
    /// construction). Rank 16.
    KeyOf {
        subject: TypeId<'db>,
    },
    /// Symbolic `value-of<T>`. Rank 17.
    ValueOf {
        subject: TypeId<'db>,
    },
    /// A conditional return type, evaluated at the call site by the
    /// call-site solver (`solver.rs`, decision 10) and by direct
    /// substitution (`substitution.rs`'s `substitute`) once the
    /// subject is decidable; an undecidable or still-symbolic subject
    /// falls back to the branch union. Rank 18.
    Conditional {
        subject: TypeId<'db>,
        matches: TypeId<'db>,
        then_branch: TypeId<'db>,
        otherwise_branch: TypeId<'db>,
        negated: bool,
    },
    /// The late-static-binding placeholders, symbolic until call-site
    /// substitution through `member_boundary_type` (decision 1). Ranks
    /// 19, 20, 21.
    SelfPlaceholder,
    ParentPlaceholder,
    StaticPlaceholder,
}

/// The opaque interned handle of one canonical type: cheap `Eq`/`Hash`
/// for early cutoff. Handle equality is structural equality because
/// every constructor canonicalizes bottom-up before interning. The id
/// never escapes the process — [`crate::stored::StoredType`] (plan 9a)
/// is the structural form that does, produced by `StoredType::of` and
/// converted back by `StoredType::to_type_id`.
#[salsa::interned(debug)]
pub struct TypeId<'db> {
    #[returns(ref)]
    pub data: TypeData<'db>,
}

/// The element-level mixed metric (design decision 12, issue #45): how
/// many structural constituent slots a type carries, and how many of
/// those slots are exactly `mixed`. See [`TypeId::element_positions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ElementPositions {
    pub total: usize,
    pub mixed: usize,
}

impl<'db> TypeId<'db> {
    /// Walks the structural constituent slots of a type, counting each
    /// as one position and how many are exactly `mixed` (decision 12):
    /// an array or list's key and value slots, a shape field's value
    /// slot, each counted as one position and then recursed into; a
    /// union's constituents are recursed into but the union node itself
    /// is never a position. Every other variant — `Mixed` itself,
    /// scalars, `Class` (its generic arguments are not positions and
    /// are not recursed into), `Callable` (its parameter and return
    /// slots are the recorded v0 exclusion), `Intersection`,
    /// `ClassString`, `KeyOf`, `ValueOf`, `Conditional`, `Template`,
    /// `EnumCase`, the placeholders — contributes zero positions and is
    /// not recursed into. `iterable<K, V>` is not special-cased: its
    /// `Union[Array<K, V>, Class("Traversable", …)]` desugaring already
    /// yields exactly the key and value slots through the `Array` arm.
    /// Types are interned and structurally finite (acyclic), so no
    /// depth guard is needed.
    pub fn element_positions(self, db: &'db dyn salsa::Database) -> ElementPositions {
        let mut positions = ElementPositions::default();
        self.walk_element_positions(db, &mut positions);
        positions
    }

    fn walk_element_positions(
        self,
        db: &'db dyn salsa::Database,
        positions: &mut ElementPositions,
    ) {
        match self.data(db) {
            TypeData::Array { key, value, .. } => {
                let key = *key;
                let value = *value;
                count_position(db, key, positions);
                count_position(db, value, positions);
                key.walk_element_positions(db, positions);
                value.walk_element_positions(db, positions);
            }
            TypeData::Shape { fields } => {
                for field in fields {
                    count_position(db, field.value, positions);
                    field.value.walk_element_positions(db, positions);
                }
            }
            TypeData::Union { constituents } => {
                for &constituent in constituents {
                    constituent.walk_element_positions(db, positions);
                }
            }
            _ => {}
        }
    }
}

/// One structural slot's contribution: always a position, and a mixed
/// one when the slot's own type is exactly `mixed`.
fn count_position<'db>(
    db: &'db dyn salsa::Database,
    of: TypeId<'db>,
    positions: &mut ElementPositions,
) {
    positions.total += 1;
    if of.is_mixed(db) {
        positions.mixed += 1;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use crate::representation::ElementPositions;
    use crate::{CallableParameter, ShapeField, ShapeKey, TypeId};

    #[test]
    fn an_array_counts_its_key_and_value_as_positions() {
        let db = TestDatabase::default();
        let of = TypeId::array(&db, TypeId::string(&db), TypeId::mixed(&db));
        assert_eq!(
            of.element_positions(&db),
            ElementPositions { total: 2, mixed: 1 }
        );
    }

    #[test]
    fn a_wholly_mixed_type_is_not_itself_a_position() {
        let db = TestDatabase::default();
        assert_eq!(
            TypeId::mixed(&db).element_positions(&db),
            ElementPositions { total: 0, mixed: 0 }
        );
    }

    #[test]
    fn a_shape_counts_each_field_value_as_a_position_but_not_its_key() {
        let db = TestDatabase::default();
        let of = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::String("a".to_owned()),
                    optional: false,
                    value: TypeId::mixed(&db),
                },
                ShapeField {
                    key: ShapeKey::String("b".to_owned()),
                    optional: false,
                    value: TypeId::int(&db),
                },
            ],
        );
        assert_eq!(
            of.element_positions(&db),
            ElementPositions { total: 2, mixed: 1 }
        );
    }

    #[test]
    fn nested_arrays_recurse_into_the_value_slot() {
        let db = TestDatabase::default();
        let inner = TypeId::array(&db, TypeId::int(&db), TypeId::mixed(&db));
        let outer = TypeId::array(&db, TypeId::int(&db), inner);
        assert_eq!(
            outer.element_positions(&db),
            ElementPositions { total: 4, mixed: 1 }
        );
    }

    #[test]
    fn a_union_is_walked_but_is_never_itself_a_position() {
        let db = TestDatabase::default();
        let of = TypeId::union(
            &db,
            [
                TypeId::array(&db, TypeId::int(&db), TypeId::mixed(&db)),
                TypeId::array(&db, TypeId::int(&db), TypeId::int(&db)),
            ],
        );
        assert_eq!(
            of.element_positions(&db),
            ElementPositions { total: 4, mixed: 1 }
        );
    }

    #[test]
    fn a_callables_parameter_and_return_slots_contribute_no_positions() {
        let db = TestDatabase::default();
        let of = TypeId::callable(
            &db,
            vec![CallableParameter {
                parameter_type: TypeId::array(&db, TypeId::int(&db), TypeId::mixed(&db)),
                optional: false,
                variadic: false,
                by_reference: false,
            }],
            TypeId::array(&db, TypeId::string(&db), TypeId::mixed(&db)),
        );
        assert_eq!(
            of.element_positions(&db),
            ElementPositions { total: 0, mixed: 0 }
        );
    }
}
