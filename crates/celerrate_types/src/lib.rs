//! The type lattice of the analysis engine: every type is interned in
//! canonical form behind the opaque [`TypeId`] handle, so equality and
//! hashing are cheap and salsa's early cutoff applies to typed results.
//! The representation is never exposed as a matchable enum: consumers
//! construct through the `TypeId` constructors and interrogate through
//! its query methods (the plugin API commitment of the parent spec).

mod construction;
mod display;
mod judgments;
mod ordering;
mod representation;
mod widening;

pub use judgments::{Nullability, Proof, assignable_to, nullability, subtype_of};
pub use representation::{CallableParameter, FloatBits, ShapeField, ShapeKey, TypeId};
pub use widening::{STRUCTURAL_DEPTH_CAP, UNION_ARITY_CAP, join, widened_literals};
