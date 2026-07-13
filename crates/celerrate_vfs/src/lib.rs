//! File loading and in-memory overlays.
//!
//! The virtual file system is the bridge between the outside world and
//! the salsa inputs: it owns the `FileId ↔ path` mapping, holds the
//! current byte contents of every known file (disk state shadowed by
//! editor-style overlays), and reports what changed so the composition
//! root can pump new states into the database. It never reads anything
//! during a query: it pushes states, salsa pulls derivations.
//!
//! Callers pass absolute, already-normalized paths to the map:
//! [`normalize_path`] produces them, and the discovery layer above
//! decides what gets walked.

mod path;
mod vfs;
mod walk;

pub use path::normalize_path;
pub use vfs::{ChangedFile, Vfs};
pub use walk::{UnreadableDirectory, Walk, enumerate_php_files};
