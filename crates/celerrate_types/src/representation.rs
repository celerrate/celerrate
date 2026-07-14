//! The lattice representation. `TypeData` stays inside this private
//! module: the public surface is the interned [`TypeId`] handle plus
//! constructors and query methods, never a matchable enum.

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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeData {
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
}

/// The opaque interned handle of one canonical type: cheap `Eq`/`Hash`
/// for early cutoff. Handle equality is structural equality because
/// every constructor canonicalizes bottom-up before interning. The id
/// never escapes the process (plan 9a serializes structurally).
#[salsa::interned(debug)]
pub struct TypeId<'db> {
    #[returns(ref)]
    pub data: TypeData,
}
