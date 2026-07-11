//! Stable declaration identity and the per-file item tree: the
//! invalidation boundary of the analysis engine. [`AstIdMap`] numbers a
//! file's declaration nodes in tree order, so an [`AstId`] survives body
//! edits; the item tree (later modules) is the range-free,
//! `Eq`-comparable projection of one file's declarations that gives
//! salsa its early cutoff.

mod ast_id;
mod index;
mod item_nodes;
mod items;
mod lookup;
mod queries;
mod resolve;
mod symbols;

pub use ast_id::{AstId, AstIdMap};
pub use index::{
    StubSymbolEntry, StubSymbolTable, SymbolEntry, SymbolTable, source_symbol_table,
    stub_symbol_table,
};
pub use items::{Declaration, DeclarationKind, ImportKind, ItemTree, UseImport};
pub use lookup::{SymbolQuery, SymbolResolution, lookup_symbol};
pub use queries::{ast_id_map, item_tree};
pub use resolve::{SymbolSources, UseTables, resolve_candidates, resolve_name};
pub use symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};
