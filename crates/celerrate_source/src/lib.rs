//! Source text primitives for the Celerrate toolchain: text sizes, ranges,
//! and line/column indexing. This is the bottom layer of the workspace:
//! it depends on no other Celerrate crate.
//!
//! Offsets and ranges are byte-based and use the `text-size` types, which
//! cap file size at 4 GiB; source-file loading (added to this crate by a
//! later plan) is responsible for rejecting larger files before offsets
//! are ever constructed.

pub use text_size::{TextRange, TextSize};

mod line_index;

pub use line_index::{LineColumn, LineIndex};
