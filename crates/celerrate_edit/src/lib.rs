//! Structured edits on the Celerrate syntax tree, compiled into the
//! deterministic, sorted, conflict-free [`TextEdit`] set that
//! suggestions transport. Two overlapping edits are an error, never a
//! silent resolution.
//!
//! [`TextEdit`]: celerrate_source::TextEdit

mod conflict;

pub use conflict::EditConflict;
