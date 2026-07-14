//! The lattice representation. `TypeData` stays inside this private
//! module: the public surface is the interned [`TypeId`] handle plus
//! constructors and query methods, never a matchable enum.

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
}

/// The opaque interned handle of one canonical type: cheap `Eq`/`Hash`
/// for early cutoff. Handle equality is structural equality because
/// every constructor canonicalizes bottom-up before interning. The id
/// never escapes the process (plan 9a serializes structurally).
#[salsa::interned(debug)]
pub struct TypeId<'db> {
    #[returns(ref)]
    pub data: TypeData<'db>,
}
