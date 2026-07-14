//! Stable declaration identity and the per-file item tree: the
//! invalidation boundary of the analysis engine. [`AstIdMap`] numbers a
//! file's declaration nodes in tree order, so an [`AstId`] survives body
//! edits; the item tree (later modules) is the range-free,
//! `Eq`-comparable projection of one file's declarations that gives
//! salsa its early cutoff.

mod ast_id;
mod cache;
mod index;
mod item_nodes;
mod items;
mod linearize;
mod lookup;
mod member_lookup;
mod members;
mod queries;
mod reference_checks;
mod references;
mod resolve;
mod revalidation;
mod symbols;
mod syntax_gating;

pub use ast_id::{AstId, AstIdMap};
pub use cache::{ArtifactCache, ArtifactCacheInput, CacheHandle};
pub use index::{
    StubSymbolEntry, StubSymbolTable, SymbolEntry, SymbolOrigin, SymbolTable, source_symbol_table,
    stub_symbol_table,
};
pub use items::{Declaration, DeclarationKind, DefineId, ImportKind, ItemTree, UseImport};
pub use linearize::{
    AncestorEdge, AncestorRelation, ClassQuery, LinearizedClass, LinearizedMember, MagicMarkers,
    MemberOrigin, linearized_class,
};
pub use lookup::{
    SymbolQuery, SymbolResolution, analyzed_file_index, lookup_class_declaration, lookup_symbol,
};
pub use member_lookup::{MemberQuery, MemberResolution, lookup_member};
pub use members::{
    ClassMembers, Member, MemberFlags, MemberKind, MemberSignature, MemberTree, ParameterSignature,
    TraitAdaptation, TraitUse, Visibility,
};
pub use queries::{ast_id_map, item_tree, member_tree, semantic_diagnostics};
pub use reference_checks::{
    ALLOCATED_IDENTIFIERS, SYMBOL_DEPRECATED, SYMBOL_NOT_AVAILABLE, SYMBOL_REMOVED, UNKNOWN_CLASS,
    UNKNOWN_CONSTANT, UNKNOWN_FUNCTION, reference_diagnostics,
};
pub use references::{Reference, collect_references};
pub use resolve::{SymbolSources, UseTables, resolve_candidates, resolve_name};
pub use revalidation::{ResolutionAnswer, ResolutionRecord, answer_of, resolution_records};
pub use symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};
pub use syntax_gating::{SYNTAX_NOT_AVAILABLE, syntax_version_diagnostics};
