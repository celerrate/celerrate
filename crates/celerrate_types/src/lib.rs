//! The type lattice of the analysis engine: every type is interned in
//! canonical form behind the opaque [`TypeId`] handle, so equality and
//! hashing are cheap and salsa's early cutoff applies to typed results.
//! The representation is never exposed as a matchable enum: consumers
//! construct through the `TypeId` constructors and interrogate through
//! its query methods (the plugin API commitment of the parent spec).

mod construction;
mod representation;

pub use representation::{FloatBits, TypeId};
