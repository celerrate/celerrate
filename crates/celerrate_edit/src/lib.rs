//! Structured edits on the Celerrate syntax tree: [`EditBuilder`]
//! expresses edits as operations on nodes and tokens and compiles them
//! into the deterministic, sorted, conflict-free [`TextEdit`] set that
//! suggestions transport, and [`apply`] splices such a set into source
//! text. Two overlapping edits are an error, never a silent resolution,
//! and an edit never touches trivia it was not aimed at.
//!
//! [`TextEdit`]: celerrate_source::TextEdit

mod apply;
mod builder;
mod conflict;

pub use apply::{ApplyError, apply};
pub use builder::{EditBuilder, EditError};
pub use conflict::{EditConflict, find_conflict};
