//! Stable declaration identity and the per-file item tree: the
//! invalidation boundary of the analysis engine. [`AstIdMap`] numbers a
//! file's declaration nodes in tree order, so an [`AstId`] survives body
//! edits; the item tree (later modules) is the range-free,
//! `Eq`-comparable projection of one file's declarations that gives
//! salsa its early cutoff.

mod ast_id;
mod item_nodes;
mod item_tree;

pub use ast_id::{AstId, AstIdMap};
pub use item_tree::{Declaration, DeclarationKind, ImportKind, ItemTree, UseImport};
