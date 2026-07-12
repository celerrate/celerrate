//! Salsa inputs and foundational queries: the base-db layer.
//!
//! This crate defines the inputs (file contents keyed by [`FileId`])
//! and the queries every layer shares. Higher-level query definitions
//! live in their domain crates; the concrete production database is
//! assembled at the composition root (the CLI binary, a later part).
//!
//! [`FileId`]: celerrate_source::FileId

mod input;
mod queries;
pub mod testing;

pub use input::{AnalyzedFileSet, SourceFile};
pub use queries::{
    ALLOCATED_IDENTIFIERS, SOURCE_TOO_LARGE, file_diagnostics, line_index, parse, source_text,
};
