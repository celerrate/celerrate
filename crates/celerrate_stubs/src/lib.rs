//! Compiled phpstorm-stubs: the embedded index of standard-library and
//! extension symbols with per-version availability metadata.
//!
//! A pinned snapshot of phpstorm-stubs is compiled by the feature-gated
//! `stub-compiler` binary (driven by `cargo xtask compile-stubs`) into a
//! committed, versioned binary blob. At runtime the embedded blob loads
//! as a high-durability salsa input and a tracked query filters it by
//! the project's PHP version range.

mod blob;
mod index;
mod symbol;

pub use blob::{
    BLOB_FORMAT_VERSION, BLOB_MAGIC, SECTION_OVERLAYS, SECTION_SIGNATURES, SECTION_SYMBOL_TABLE,
    StubBlobError, decode, encode,
};
pub use index::StubIndex;
pub use symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};
