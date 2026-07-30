//! The structural serialization of the type lattice: [`crate::TypeId`] is
//! a process-local interner handle and never hits disk; a persisted
//! type is this self-contained mirror, re-interned
//! through the public constructors on the way back in — which
//! canonicalize, so a forged or stale value re-canonicalizes instead of
//! panicking.

use celerrate_db::ContentHash;
use serde::{Deserialize, Serialize};

use crate::TypeId;
use crate::declared::{DeclaredSignature, Trust};
use crate::representation::{CallableParameter, ShapeField, ShapeKey, StringConstraint, TypeData};

/// The nesting-depth budget shared by [`StoredType::to_type_id`] and the
/// manual `Deserialize` implementation (the decode guard below).
/// Generous headroom above the live lattice's construction-time depth
/// cap (`crate::widening::STRUCTURAL_DEPTH_CAP`, 16), while remaining
/// small enough that postcard's per-level recursion on either side of
/// the wire cannot overflow the stack before the guard trips: `serde`
/// gives us no way to thread a depth counter through the standard
/// `Deserialize` trait, so both guards check depth first and recurse
/// second, on every call, rather than only after the whole tree exists.
pub const STORED_DEPTH_LIMIT: usize = 64;

/// One structurally serialized type. One variant per [`TypeData`]
/// variant; `TypeId` fields become `Box<StoredType>` / `Vec<StoredType>`.
/// `Deserialize` is a manual, depth-counting implementation (see
/// [`STORED_DEPTH_LIMIT`]); everything else derives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum StoredType {
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
    /// The `FloatBits` bit pattern, stored raw so NaN canonicalization
    /// and the `0.0`/`-0.0` distinction survive the round trip.
    Float {
        literal: Option<u64>,
    },
    String {
        constraint: StoredStringConstraint,
    },
    Union {
        constituents: Vec<StoredType>,
    },
    Intersection {
        intersectands: Vec<StoredType>,
    },
    Array {
        key: Box<StoredType>,
        value: Box<StoredType>,
        is_list: bool,
        non_empty: bool,
    },
    Shape {
        fields: Vec<StoredShapeField>,
    },
    ClassString {
        argument: Option<Box<StoredType>>,
    },
    Class {
        name: String,
        arguments: Vec<StoredType>,
    },
    EnumCase {
        enum_name: String,
        case_name: String,
    },
    Callable {
        parameters: Vec<StoredCallableParameter>,
        return_type: Box<StoredType>,
    },
    Template {
        scope: String,
        name: String,
        bound: Box<StoredType>,
    },
    KeyOf {
        subject: Box<StoredType>,
    },
    ValueOf {
        subject: Box<StoredType>,
    },
    Conditional {
        subject: Box<StoredType>,
        matches: Box<StoredType>,
        then_branch: Box<StoredType>,
        otherwise_branch: Box<StoredType>,
        negated: bool,
    },
    SelfPlaceholder,
    ParentPlaceholder,
    StaticPlaceholder,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StoredStringConstraint {
    General,
    NonEmpty,
    Numeric,
    LiteralMarker,
    Literal(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredShapeField {
    pub key: StoredShapeKey,
    pub optional: bool,
    pub value: StoredType,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StoredShapeKey {
    Integer(i64),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredCallableParameter {
    pub parameter_type: StoredType,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}

/// The structural mirror of [`Trust`]: how one
/// declared element's final type was obtained. Digested alongside its
/// type, so an annotation-layer change that only flips the trust verdict
/// (a `RejectedAnnotation` becoming `Refined` with the SAME resolved
/// type, for instance) still flips the class-surface digest — the
/// verdict is itself judgment-visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StoredTrust {
    NativeOnly,
    Refined,
    RefinedUnproven,
    RejectedAnnotation,
}

impl StoredTrust {
    pub fn of(trust: Trust) -> StoredTrust {
        match trust {
            Trust::NativeOnly => StoredTrust::NativeOnly,
            Trust::Refined => StoredTrust::Refined,
            Trust::RefinedUnproven => StoredTrust::RefinedUnproven,
            Trust::RejectedAnnotation => StoredTrust::RejectedAnnotation,
        }
    }
}

/// The structural mirror of one [`crate::declared::DeclaredParameter`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredParameter {
    pub name: String,
    /// `None` mirrors the empty-intersection stub guard verbatim: it
    /// is a judgment-visible fact, so it participates in the digest.
    pub parameter_type: Option<StoredType>,
    pub trust: StoredTrust,
    pub optional: bool,
    pub variadic: bool,
    pub by_reference: bool,
}

/// The structural mirror of one resolved
/// [`crate::declared::DeclaredSignature`]: the persisted (and digested)
/// shape of a member's or a function's whole declared signature.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredSignature {
    pub parameters: Vec<StoredParameter>,
    pub value_type: StoredType,
    pub value_trust: StoredTrust,
    pub by_reference: bool,
}

impl StoredSignature {
    /// Mirrors one resolved `DeclaredSignature`'s data into the
    /// structural form, through [`StoredType::of`] exclusively — `TypeId`
    /// never enters a stored shape directly.
    pub fn of<'db>(
        db: &'db dyn salsa::Database,
        signature: &DeclaredSignature<'db>,
    ) -> StoredSignature {
        StoredSignature {
            parameters: signature
                .parameters
                .iter()
                .map(|parameter| StoredParameter {
                    name: parameter.name.clone(),
                    parameter_type: parameter
                        .parameter_type
                        .map(|type_id| StoredType::of(db, type_id)),
                    trust: StoredTrust::of(parameter.trust),
                    optional: parameter.optional,
                    variadic: parameter.variadic,
                    by_reference: parameter.by_reference,
                })
                .collect(),
            value_type: StoredType::of(db, signature.value_type),
            value_trust: StoredTrust::of(signature.value_trust),
            by_reference: signature.by_reference,
        }
    }
}

/// The persisted key of one inferred signature: a
/// free function through its folded Function-space key, or a method
/// through its enclosing class-like's folded key plus the member's own
/// folded key — the same two-part identity [`crate::inference::BodyOwner`]
/// carries, reconstructed here from public projections
/// (`celerrate_semantics::member_tree`, `folded_symbol_key`,
/// `folded_member_key`) rather than that crate-private type, so the
/// persist path in `celerrate_cli` never needs it visible. Derives
/// `Ord` so the persist path can sort entries into a deterministic pack
/// order, keeping the first of any duplicate key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StoredSignatureKey {
    Function {
        key: String,
    },
    Method {
        class_key: String,
        member_key: String,
    },
}

/// One class a recorded body's flow walk consulted
/// (`TypedDependencies::classes`), alongside its class-surface digest at
/// persist time: revalidation works by recomputing the live digest
/// through [`crate::records::class_surface_digest`] and comparing.
/// `digest` is `None` when the digest query itself answered `None` (the
/// key no longer names a source class-like) — a recordable fact, not a
/// missing one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredClassDependency {
    pub key: String,
    pub digest: Option<[u8; 32]>,
}

/// The free-function sibling of [`StoredClassDependency`]: one
/// function-space key a recorded body consulted through the DECLARED
/// tier (`TypedDependencies::functions`), and its signature
/// digest at persist time through
/// [`crate::records::function_signature_digest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFunctionDependency {
    pub key: String,
    pub digest: Option<[u8; 32]>,
}

/// One call a recorded body resolved through the INFERRED tier: the
/// callee's own persisted key, and the raw pre-substitution return type
/// the walk actually consumed. Mirrors
/// [`crate::records::TypedDependencies`]'s own raw-answer invariant
/// (its rustdoc) — recorded before any call-site substitution, so
/// revalidation re-invokes the same callee query and compares the
/// same, unsubstituted vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredInferredEdge {
    pub callee: StoredSignatureKey,
    pub return_type: StoredType,
}

/// One body's persisted inferred signature. The
/// defining file's content hash stands in for body identity — this
/// keeps the warm serve path from ever reading a body IR —
/// and every class, function, and inferred-tier callee the body's flow
/// walk actually consulted is carried verbatim from
/// `TypedDependencies`/`FileDependencies`, never re-derived by a
/// separate mirror walk.
///
/// Persisted unconditionally: an annotated body (a declared return
/// type or a proven `@return`) still carries an inferred return here —
/// the artifact itself never depends on whether a declaration exists.
/// Only the EDGES *into* this record are what a declared return cuts
/// (a caller whose callee has a declared return consults the declared
/// tier, `StoredFunctionDependency`/`StoredClassDependency`, never
/// `StoredInferredEdge`), never this record's own existence.
///
/// Recorded coarsening: a finer-grained scheme would key a
/// signature "by body content"; this record keys by the *defining
/// file's* content hash instead — any edit anywhere in the file
/// invalidates every one of its bodies' persisted signatures, not just
/// the edited body's. Sound (consistent with every other pack key,
/// which is file-grained) and cheap, and the early-cutoff loss it costs
/// is bounded by the cross-boundary cutoff (a recomputed body
/// whose return still matches still validates its callers). Measured
/// corpus numbers own whether the coarseness matters enough to key
/// per body instead — a pack-only change behind this same record shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredInferredSignature {
    /// The defining file at persist time: body identity by proxy.
    pub content: ContentHash,
    pub return_type: StoredType,
    pub classes: Vec<StoredClassDependency>,
    pub functions: Vec<StoredFunctionDependency>,
    pub inferred: Vec<StoredInferredEdge>,
}

/// blake3 over the postcard encoding; `None` when encoding fails (never
/// observed for these plain shapes, but the zero-panic rule forbids
/// assuming it).
pub(crate) fn digest_of<T: Serialize>(value: &T) -> Option<[u8; 32]> {
    let bytes = postcard::to_allocvec(value).ok()?;
    Some(*blake3::hash(&bytes).as_bytes())
}

// --- Deserialize: a manual, depth-counting implementation -----------
//
// `serde`'s `Deserialize` trait carries no side channel for extra
// state, so the depth budget rides a thread-local counter instead: an
// RAII guard increments it on entry to every `StoredType::deserialize`
// call (the only place recursion happens, since every nested
// `Box<StoredType>` / `Vec<StoredType>` field routes back through this
// same entry point) and decrements it on exit, checking the budget
// *before* doing any further work so a hostile byte stream is rejected
// before the recursion that would read it ever starts.

thread_local! {
    static DESERIALIZE_DEPTH: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

struct DepthGuard;

impl DepthGuard {
    fn enter<E>() -> Result<Self, E>
    where
        E: serde::de::Error,
    {
        let depth = DESERIALIZE_DEPTH.with(core::cell::Cell::get);
        if depth > STORED_DEPTH_LIMIT {
            return Err(E::custom("stored type nesting exceeds STORED_DEPTH_LIMIT"));
        }
        DESERIALIZE_DEPTH.with(|cell| cell.set(depth + 1));
        Ok(DepthGuard)
    }
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        DESERIALIZE_DEPTH.with(|cell| cell.set(cell.get().saturating_sub(1)));
    }
}

/// The derive template for [`StoredType`]'s shape, wired through
/// `#[serde(remote = "StoredType")]` so the generated implementation
/// deserializes directly into the real type: every nested
/// `Box<StoredType>` / `Vec<StoredType>` field this template lists
/// deserializes through `StoredType`'s own (guarded) `Deserialize`
/// implementation below, not through this template a second time.
#[derive(Deserialize)]
#[serde(remote = "StoredType")]
enum StoredTypeShape {
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
        literal: Option<u64>,
    },
    String {
        constraint: StoredStringConstraint,
    },
    Union {
        constituents: Vec<StoredType>,
    },
    Intersection {
        intersectands: Vec<StoredType>,
    },
    Array {
        key: Box<StoredType>,
        value: Box<StoredType>,
        is_list: bool,
        non_empty: bool,
    },
    Shape {
        fields: Vec<StoredShapeField>,
    },
    ClassString {
        argument: Option<Box<StoredType>>,
    },
    Class {
        name: String,
        arguments: Vec<StoredType>,
    },
    EnumCase {
        enum_name: String,
        case_name: String,
    },
    Callable {
        parameters: Vec<StoredCallableParameter>,
        return_type: Box<StoredType>,
    },
    Template {
        scope: String,
        name: String,
        bound: Box<StoredType>,
    },
    KeyOf {
        subject: Box<StoredType>,
    },
    ValueOf {
        subject: Box<StoredType>,
    },
    Conditional {
        subject: Box<StoredType>,
        matches: Box<StoredType>,
        then_branch: Box<StoredType>,
        otherwise_branch: Box<StoredType>,
        negated: bool,
    },
    SelfPlaceholder,
    ParentPlaceholder,
    StaticPlaceholder,
}

impl<'de> Deserialize<'de> for StoredType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let _guard = DepthGuard::enter::<D::Error>()?;
        StoredTypeShape::deserialize(deserializer)
    }
}

// --- construction (`of`) and reconstruction (`to_type_id`) ----------

impl StoredType {
    /// Mirrors `of`'s data into the structural form. Matches
    /// `of.data(db)` exhaustively: this module lives in the owning
    /// crate, where [`TypeData`] is matchable (the no-matchable-enum
    /// commitment binds plugins, not the crate itself).
    pub fn of<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> StoredType {
        match of.data(db) {
            TypeData::Mixed => StoredType::Mixed,
            TypeData::Never => StoredType::Never,
            TypeData::Void => StoredType::Void,
            TypeData::Null => StoredType::Null,
            TypeData::Object => StoredType::Object,
            TypeData::Resource => StoredType::Resource,
            TypeData::Bool { literal } => StoredType::Bool { literal: *literal },
            TypeData::Int { minimum, maximum } => StoredType::Int {
                minimum: *minimum,
                maximum: *maximum,
            },
            TypeData::Float { literal } => StoredType::Float {
                literal: literal.map(|bits| bits.value().to_bits()),
            },
            TypeData::String { constraint } => StoredType::String {
                constraint: StoredStringConstraint::of(constraint),
            },
            TypeData::Union { constituents } => StoredType::Union {
                constituents: constituents
                    .iter()
                    .map(|part| StoredType::of(db, *part))
                    .collect(),
            },
            TypeData::Intersection { intersectands } => StoredType::Intersection {
                intersectands: intersectands
                    .iter()
                    .map(|part| StoredType::of(db, *part))
                    .collect(),
            },
            TypeData::Array {
                key,
                value,
                is_list,
                non_empty,
            } => StoredType::Array {
                key: Box::new(StoredType::of(db, *key)),
                value: Box::new(StoredType::of(db, *value)),
                is_list: *is_list,
                non_empty: *non_empty,
            },
            TypeData::Shape { fields } => StoredType::Shape {
                fields: fields
                    .iter()
                    .map(|field| StoredShapeField::of(db, field))
                    .collect(),
            },
            TypeData::ClassString { argument } => StoredType::ClassString {
                argument: (*argument).map(|inner| Box::new(StoredType::of(db, inner))),
            },
            TypeData::Class { name, arguments } => StoredType::Class {
                name: name.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| StoredType::of(db, *argument))
                    .collect(),
            },
            TypeData::EnumCase {
                enum_name,
                case_name,
            } => StoredType::EnumCase {
                enum_name: enum_name.clone(),
                case_name: case_name.clone(),
            },
            TypeData::Callable {
                parameters,
                return_type,
            } => StoredType::Callable {
                parameters: parameters
                    .iter()
                    .map(|parameter| StoredCallableParameter::of(db, parameter))
                    .collect(),
                return_type: Box::new(StoredType::of(db, *return_type)),
            },
            TypeData::Template { scope, name, bound } => StoredType::Template {
                scope: scope.clone(),
                name: name.clone(),
                bound: Box::new(StoredType::of(db, *bound)),
            },
            TypeData::KeyOf { subject } => StoredType::KeyOf {
                subject: Box::new(StoredType::of(db, *subject)),
            },
            TypeData::ValueOf { subject } => StoredType::ValueOf {
                subject: Box::new(StoredType::of(db, *subject)),
            },
            TypeData::Conditional {
                subject,
                matches,
                then_branch,
                otherwise_branch,
                negated,
            } => StoredType::Conditional {
                subject: Box::new(StoredType::of(db, *subject)),
                matches: Box::new(StoredType::of(db, *matches)),
                then_branch: Box::new(StoredType::of(db, *then_branch)),
                otherwise_branch: Box::new(StoredType::of(db, *otherwise_branch)),
                negated: *negated,
            },
            TypeData::SelfPlaceholder => StoredType::SelfPlaceholder,
            TypeData::ParentPlaceholder => StoredType::ParentPlaceholder,
            TypeData::StaticPlaceholder => StoredType::StaticPlaceholder,
        }
    }

    /// Re-interns through the public constructors, which canonicalize:
    /// a forged or stale value re-canonicalizes instead of panicking.
    /// Answers `None` past [`STORED_DEPTH_LIMIT`] (a silent cache miss,
    /// never a panic, never a stack overflow).
    pub fn to_type_id<'db>(&self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        self.to_type_id_within(db, 0)
    }

    fn to_type_id_within<'db>(
        &self,
        db: &'db dyn salsa::Database,
        depth: usize,
    ) -> Option<TypeId<'db>> {
        if depth > STORED_DEPTH_LIMIT {
            return None;
        }
        let next_depth = depth + 1;
        match self {
            StoredType::Mixed => Some(TypeId::mixed(db)),
            StoredType::Never => Some(TypeId::never(db)),
            StoredType::Void => Some(TypeId::void(db)),
            StoredType::Null => Some(TypeId::null(db)),
            StoredType::Object => Some(TypeId::object(db)),
            StoredType::Resource => Some(TypeId::resource(db)),
            StoredType::Bool { literal } => Some(match literal {
                Some(value) => TypeId::bool_literal(db, *value),
                None => TypeId::bool(db),
            }),
            StoredType::Int { minimum, maximum } => Some(TypeId::int_range(db, *minimum, *maximum)),
            StoredType::Float { literal } => Some(match literal {
                Some(bits) => TypeId::float_literal(db, f64::from_bits(*bits)),
                None => TypeId::float(db),
            }),
            StoredType::String { constraint } => Some(match constraint {
                StoredStringConstraint::General => TypeId::string(db),
                StoredStringConstraint::NonEmpty => TypeId::non_empty_string(db),
                StoredStringConstraint::Numeric => TypeId::numeric_string(db),
                StoredStringConstraint::LiteralMarker => TypeId::literal_string_type(db),
                StoredStringConstraint::Literal(value) => TypeId::string_literal(db, value),
            }),
            StoredType::Union { constituents } => {
                let parts = constituents
                    .iter()
                    .map(|part| part.to_type_id_within(db, next_depth))
                    .collect::<Option<Vec<_>>>()?;
                Some(TypeId::union(db, parts))
            }
            StoredType::Intersection { intersectands } => {
                let parts = intersectands
                    .iter()
                    .map(|part| part.to_type_id_within(db, next_depth))
                    .collect::<Option<Vec<_>>>()?;
                Some(TypeId::intersection(db, parts))
            }
            StoredType::Array {
                key,
                value,
                is_list,
                non_empty,
            } => {
                let value_id = value.to_type_id_within(db, next_depth)?;
                Some(if *is_list {
                    if *non_empty {
                        TypeId::non_empty_list(db, value_id)
                    } else {
                        TypeId::list(db, value_id)
                    }
                } else {
                    let key_id = key.to_type_id_within(db, next_depth)?;
                    if *non_empty {
                        TypeId::non_empty_array(db, key_id, value_id)
                    } else {
                        TypeId::array(db, key_id, value_id)
                    }
                })
            }
            StoredType::Shape { fields } => {
                let fields = fields
                    .iter()
                    .map(|field| field.to_shape_field(db, next_depth))
                    .collect::<Option<Vec<_>>>()?;
                Some(TypeId::shape(db, fields))
            }
            StoredType::ClassString { argument } => {
                let argument = match argument {
                    Some(inner) => Some(inner.to_type_id_within(db, next_depth)?),
                    None => None,
                };
                Some(TypeId::class_string(db, argument))
            }
            StoredType::Class { name, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| argument.to_type_id_within(db, next_depth))
                    .collect::<Option<Vec<_>>>()?;
                Some(TypeId::class(db, name, arguments))
            }
            StoredType::EnumCase {
                enum_name,
                case_name,
            } => Some(TypeId::enum_case(db, enum_name, case_name)),
            StoredType::Callable {
                parameters,
                return_type,
            } => {
                let parameters = parameters
                    .iter()
                    .map(|parameter| parameter.to_callable_parameter(db, next_depth))
                    .collect::<Option<Vec<_>>>()?;
                let return_type = return_type.to_type_id_within(db, next_depth)?;
                Some(TypeId::callable(db, parameters, return_type))
            }
            StoredType::Template { scope, name, bound } => {
                let bound = bound.to_type_id_within(db, next_depth)?;
                Some(TypeId::template(db, scope, name, bound))
            }
            StoredType::KeyOf { subject } => {
                let subject = subject.to_type_id_within(db, next_depth)?;
                Some(TypeId::key_of(db, subject))
            }
            StoredType::ValueOf { subject } => {
                let subject = subject.to_type_id_within(db, next_depth)?;
                Some(TypeId::value_of(db, subject))
            }
            StoredType::Conditional {
                subject,
                matches,
                then_branch,
                otherwise_branch,
                negated,
            } => {
                let subject = subject.to_type_id_within(db, next_depth)?;
                let matches = matches.to_type_id_within(db, next_depth)?;
                let then_branch = then_branch.to_type_id_within(db, next_depth)?;
                let otherwise_branch = otherwise_branch.to_type_id_within(db, next_depth)?;
                Some(TypeId::conditional(
                    db,
                    subject,
                    matches,
                    then_branch,
                    otherwise_branch,
                    *negated,
                ))
            }
            StoredType::SelfPlaceholder => Some(TypeId::self_placeholder(db)),
            StoredType::ParentPlaceholder => Some(TypeId::parent_placeholder(db)),
            StoredType::StaticPlaceholder => Some(TypeId::static_placeholder(db)),
        }
    }
}

impl StoredStringConstraint {
    fn of(constraint: &StringConstraint) -> StoredStringConstraint {
        match constraint {
            StringConstraint::General => StoredStringConstraint::General,
            StringConstraint::NonEmpty => StoredStringConstraint::NonEmpty,
            StringConstraint::Numeric => StoredStringConstraint::Numeric,
            StringConstraint::LiteralMarker => StoredStringConstraint::LiteralMarker,
            StringConstraint::Literal(value) => StoredStringConstraint::Literal(value.clone()),
        }
    }
}

impl StoredShapeField {
    fn of<'db>(db: &'db dyn salsa::Database, field: &ShapeField<'db>) -> StoredShapeField {
        StoredShapeField {
            key: StoredShapeKey::of(&field.key),
            optional: field.optional,
            value: StoredType::of(db, field.value),
        }
    }

    fn to_shape_field<'db>(
        &self,
        db: &'db dyn salsa::Database,
        depth: usize,
    ) -> Option<ShapeField<'db>> {
        let value = self.value.to_type_id_within(db, depth)?;
        Some(ShapeField {
            key: self.key.to_shape_key(),
            optional: self.optional,
            value,
        })
    }
}

impl StoredShapeKey {
    fn of(key: &ShapeKey) -> StoredShapeKey {
        match key {
            ShapeKey::Integer(value) => StoredShapeKey::Integer(*value),
            ShapeKey::String(value) => StoredShapeKey::String(value.clone()),
        }
    }

    fn to_shape_key(&self) -> ShapeKey {
        match self {
            StoredShapeKey::Integer(value) => ShapeKey::Integer(*value),
            StoredShapeKey::String(value) => ShapeKey::String(value.clone()),
        }
    }
}

impl StoredCallableParameter {
    fn of<'db>(
        db: &'db dyn salsa::Database,
        parameter: &CallableParameter<'db>,
    ) -> StoredCallableParameter {
        StoredCallableParameter {
            parameter_type: StoredType::of(db, parameter.parameter_type),
            optional: parameter.optional,
            variadic: parameter.variadic,
            by_reference: parameter.by_reference,
        }
    }

    fn to_callable_parameter<'db>(
        &self,
        db: &'db dyn salsa::Database,
        depth: usize,
    ) -> Option<CallableParameter<'db>> {
        let parameter_type = self.parameter_type.to_type_id_within(db, depth)?;
        Some(CallableParameter {
            parameter_type,
            optional: self.optional,
            variadic: self.variadic,
            by_reference: self.by_reference,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;

    use super::*;
    use crate::{CallableParameter, ShapeField, ShapeKey, TypeId};

    #[test]
    fn every_ground_shape_round_trips() {
        let db = TestDatabase::default();
        let samples = vec![
            TypeId::mixed(&db),
            TypeId::never(&db),
            TypeId::void(&db),
            TypeId::null(&db),
            TypeId::object(&db),
            TypeId::resource(&db),
            TypeId::bool(&db),
            TypeId::bool_literal(&db, true),
            TypeId::int(&db),
            TypeId::int_literal(&db, 42),
            TypeId::int_range(&db, Some(1), None),
            TypeId::float(&db),
            TypeId::float_literal(&db, 1.5),
            TypeId::string(&db),
            TypeId::non_empty_string(&db),
            TypeId::numeric_string(&db),
            TypeId::literal_string_type(&db),
            TypeId::string_literal(&db, "active"),
            TypeId::class(&db, "app\\user", vec![]),
            TypeId::enum_case(&db, "app\\status", "Active"),
            TypeId::class_string(&db, None),
        ];
        for sample in samples {
            let stored = StoredType::of(&db, sample);
            assert_eq!(stored.to_type_id(&db), Some(sample));
        }
    }

    #[test]
    fn every_composite_shape_round_trips() {
        let db = TestDatabase::default();
        let user = TypeId::class(&db, "app\\user", vec![]);
        let nullable = TypeId::union(&db, [user, TypeId::null(&db)]);
        let samples = vec![
            nullable,
            TypeId::intersection(&db, [user, TypeId::class(&db, "countable", vec![])]),
            TypeId::array(&db, TypeId::string(&db), nullable),
            TypeId::list(&db, user),
            TypeId::non_empty_array(&db, TypeId::int(&db), user),
            TypeId::shape(
                &db,
                vec![
                    ShapeField {
                        key: ShapeKey::String("id".to_owned()),
                        optional: false,
                        value: TypeId::int(&db),
                    },
                    ShapeField {
                        key: ShapeKey::Integer(0),
                        optional: true,
                        value: nullable,
                    },
                ],
            ),
            TypeId::class(&db, "collection", vec![user]),
            TypeId::class_string(&db, Some(user)),
            TypeId::callable(
                &db,
                vec![CallableParameter {
                    parameter_type: user,
                    optional: false,
                    variadic: false,
                    by_reference: false,
                }],
                nullable,
            ),
        ];
        for sample in samples {
            let stored = StoredType::of(&db, sample);
            assert_eq!(stored.to_type_id(&db), Some(sample));
        }
    }

    #[test]
    fn every_symbolic_shape_round_trips() {
        // Symbolic forms survive into persisted inferred returns: the
        // serialization cannot assume ground types. Both
        // `key_of` and `value_of` evaluate decidable subjects at
        // construction (a shape, an array), so an undecidable subject
        // (a template) is required to actually exercise the symbolic
        // `KeyOf`/`ValueOf` variants here, matching
        // `construction.rs`'s own `key_of_and_value_of_...` test.
        let db = TestDatabase::default();
        let bound = TypeId::class(&db, "app\\entity", vec![]);
        let template = TypeId::template(&db, "app\\repo::find", "T", bound);
        let samples = vec![
            template,
            TypeId::key_of(&db, template),
            TypeId::value_of(&db, template),
            TypeId::conditional(&db, template, bound, TypeId::null(&db), bound, false),
            TypeId::static_placeholder(&db),
            TypeId::self_placeholder(&db),
            TypeId::parent_placeholder(&db),
        ];
        for sample in samples {
            let stored = StoredType::of(&db, sample);
            assert_eq!(stored.to_type_id(&db), Some(sample));
        }
    }

    #[test]
    fn a_forged_non_canonical_value_re_canonicalizes_instead_of_panicking() {
        // A hand-written pack can carry anything: a one-armed union,
        // a duplicated constituent. Re-interning goes through the
        // constructors, which canonicalize — never a panic, never a
        // non-canonical handle.
        let db = TestDatabase::default();
        let one_armed = StoredType::Union {
            constituents: vec![StoredType::Int {
                minimum: None,
                maximum: None,
            }],
        };
        assert_eq!(one_armed.to_type_id(&db), Some(TypeId::int(&db)));
        let duplicated = StoredType::Union {
            constituents: vec![
                StoredType::Null,
                StoredType::Null,
                StoredType::Int {
                    minimum: None,
                    maximum: None,
                },
            ],
        };
        assert_eq!(
            duplicated.to_type_id(&db),
            Some(TypeId::union(&db, [TypeId::null(&db), TypeId::int(&db)]))
        );
        let empty = StoredType::Union {
            constituents: vec![],
        };
        // An empty union has no constructible meaning: the defined
        // degenerate answer is `never` (the union identity).
        assert_eq!(empty.to_type_id(&db), Some(TypeId::never(&db)));
    }

    #[test]
    fn an_over_deep_value_is_a_silent_miss_never_an_overflow() {
        // Nesting past STORED_DEPTH_LIMIT is forged with an iterative
        // fold (the test itself never recurses): to_type_id answers
        // None, never a panic, never a stack overflow. The
        // Deserialize half of the guard is pinned at the byte level
        // in a dedicated adversarial test suite elsewhere; a
        // lightweight sanity check of that half lives just below.
        let db = TestDatabase::default();
        let mut deep = StoredType::Null;
        for _ in 0..=STORED_DEPTH_LIMIT {
            deep = StoredType::KeyOf {
                subject: Box::new(deep),
            };
        }
        assert_eq!(deep.to_type_id(&db), None);
    }

    #[test]
    fn an_over_deep_encoded_value_is_a_deserialize_error_never_an_overflow() {
        // The Deserialize-side guard must reject the same
        // over-deep shape before it recurses into it, not merely once
        // it is fully materialized: encode the forged value, then
        // decode it back, and expect a clean error.
        let mut deep = StoredType::Null;
        for _ in 0..=STORED_DEPTH_LIMIT {
            deep = StoredType::KeyOf {
                subject: Box::new(deep),
            };
        }
        let bytes = postcard::to_allocvec(&deep).unwrap();
        assert!(postcard::from_bytes::<StoredType>(&bytes).is_err());
    }

    #[test]
    fn the_encoding_is_deterministic() {
        let db = TestDatabase::default();
        let user = TypeId::class(&db, "app\\user", vec![]);
        let value = TypeId::union(&db, [user, TypeId::null(&db), TypeId::int(&db)]);
        let first = postcard::to_allocvec(&StoredType::of(&db, value)).unwrap();
        let second = postcard::to_allocvec(&StoredType::of(&db, value)).unwrap();
        assert_eq!(first, second);
    }
}
