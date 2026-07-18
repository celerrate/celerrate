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
