//! Stable declaration identity and the per-file item tree: the
//! invalidation boundary of the analysis engine. [`AstIdMap`] numbers a
//! file's declaration nodes in tree order, so an [`AstId`] survives body
//! edits; the item tree (later modules) is the range-free,
//! `Eq`-comparable projection of one file's declarations that gives
//! salsa its early cutoff.
//!
//! One level down, the body IR (`body_ir`) lowers each function or
//! method body into a range-free arena behind the same split: spans
//! reconcile late through `body_source_map`, and only code plus
//! recognized annotation content invalidates body consumers.

mod ast_id;
mod body;
mod body_lowering;
mod cache;
mod comment_directives;
mod index;
mod item_nodes;
mod items;
mod linearize;
mod lookup;
mod member_lookup;
mod members;
mod plugin;
mod queries;
mod reference_checks;
mod references;
mod resolve;
mod revalidation;
mod rule_context;
mod strict_types;
mod symbols;
mod syntax_gating;
mod virtual_symbols;

pub use ast_id::{AstId, AstIdMap};
pub use body::{
    ArrayEntry, BodyAnnotation, BodyExpression, BodyIr, BodyQuery, BodySourceMap, BodyStatement,
    CallArgument, CatchArm, ClassReference, ClosureUse, ExpressionId, MatchCase, MemberReference,
    StatementId, StaticVariableDeclaration, StringPart, SwitchArm, body_ir, body_source_map,
    is_recognized_annotation,
};
pub use cache::{ArtifactCache, ArtifactCacheInput, CacheHandle};
pub use comment_directives::{
    CommentDirective, CommentDirectiveProvider, CommentDirectiveRegistration,
    CommentDirectiveRegistry, CommentKind, DirectiveScope, is_suppressed, suppressed_ranges,
};
pub use index::{
    StubFrontier, StubSignatureTable, StubSymbolEntry, StubSymbolTable, SymbolEntry, SymbolOrigin,
    SymbolTable, source_symbol_table, stub_ancestors_of, stub_frontier, stub_signature_table,
    stub_symbol_table,
};
pub use items::{Declaration, DeclarationKind, DefineId, ImportKind, ItemTree, UseImport};
pub use linearize::{
    AncestorEdge, AncestorRelation, ClassQuery, LinearizedClass, LinearizedMember,
    LinearizedVirtualMember, MagicMarkers, MemberOrigin, anonymous_class_key,
    class_declaration_kind, folded_member_key, linearized_class, parse_anonymous_class_key,
};
pub use lookup::{
    SymbolQuery, SymbolResolution, analyzed_file_index, lookup_class_declaration,
    lookup_function_declaration, lookup_symbol,
};
pub use member_lookup::{
    ClassSurface, MemberQuery, MemberResolution, class_surface, lookup_member,
};
pub use members::{
    ClassMembers, FreeFunction, Member, MemberFlags, MemberKind, MemberSignature, MemberTree,
    ParameterSignature, TraitAdaptation, TraitUse, Visibility,
};
pub use plugin::PluginIdentity;
pub use queries::{ast_id_map, item_tree, member_tree, semantic_diagnostics};
pub use reference_checks::{
    ALLOCATED_IDENTIFIERS, ReferenceOutcome, ReferenceOutcomes, ResolutionOutcome,
    SYMBOL_DEPRECATED, SYMBOL_NOT_AVAILABLE, SYMBOL_REMOVED, reference_diagnostics,
    reference_outcomes, reference_resolutions,
};
pub use references::{Reference, collect_references};
pub use resolve::{SymbolSources, UseTables, resolve_candidates, resolve_name};
pub use revalidation::{ResolutionAnswer, ResolutionRecord, answer_of, resolution_records};
pub use rule_context::{SemanticContext, semantic_context};
pub use strict_types::file_strict_types;
pub use symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};
pub use syntax_gating::{GatedSyntaxUse, gated_syntax_uses};
pub use virtual_symbols::{
    VirtualMember, VirtualMemberKind, VirtualParameter, VirtualSymbolProvider,
    VirtualSymbolRegistration, VirtualSymbolRegistry,
};

pub use celerrate_stubs::{StubAvailability, StubDeprecation};
