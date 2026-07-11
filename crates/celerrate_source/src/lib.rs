//! Source text primitives for the Celerrate toolchain: file identifiers,
//! decoded source text, text sizes, ranges, and line/column indexing.
//! This is the bottom layer of the workspace: it depends on no other
//! Celerrate crate and performs no I/O: file contents arrive as bytes
//! from whoever discovers files (command-line walk, editor buffers,
//! tests) and are decoded by [`SourceText::from_bytes`].
//!
//! Offsets and ranges are byte-based and use the `text-size` types, which
//! cap file size at 4 GiB; decoding rejects larger inputs (as
//! [`SourceTooLarge`]) before offsets are ever constructed.

pub use text_size::{TextRange, TextSize};

mod file_id;
mod line_index;
mod source_text;

pub use file_id::FileId;
pub use line_index::{LineColumn, LineIndex};
pub use source_text::{SourceText, SourceTooLarge};
