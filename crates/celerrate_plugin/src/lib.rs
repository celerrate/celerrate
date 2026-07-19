//! The plugin API facade. Extension points are owned by their
//! consuming layers (`celerrate_types`, `celerrate_semantics`); this
//! crate aggregates and re-exports the stable surface so a plugin
//! crate declares exactly one dependency. Implementations are
//! constructed and registered at the composition root
//! (`celerrate_cli`). An extension point that proves insufficient is
//! extended, never bypassed.
//!
//! The API is deliberately not called v1: its second *dissimilar*
//! consumer (a framework dynamic type provider) is sub-project 6.

/// The API version the composition root checks at registration. A
/// mismatch excludes the plugin for the whole run and the run is
/// reported degraded. For compiled-in first-party plugins the check
/// cannot fail — dormant scaffolding whose first real exercise is
/// the WASM host. Distinct from the plugin version inside
/// `PluginIdentity`; only the latter keys the cache.
pub const PLUGIN_API_VERSION: u32 = 0;

/// What a plugin exposes for registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDescriptor {
    pub identity: PluginIdentity,
    pub api_version: u32,
}

// Identity and the comment-directive and virtual-symbol extension points.
pub use celerrate_semantics::{
    CommentDirective, CommentDirectiveProvider, CommentKind, DirectiveScope, PluginIdentity,
    VirtualMember, VirtualMemberKind, VirtualParameter, VirtualSymbolProvider,
};

// The type-syntax and dynamic-type-provider extension points, and the
// type vocabulary plugins construct and interrogate through. Nominal
// re-exports only: never the database crate, never a whole crate — the boundary
// surface is enumerable by reading this list.
pub use celerrate_types::{
    AnnotationSite, AssertionPolarity, CallableParameter, DynamicTypeProvider, InvocationSite,
    ParsedAncestor, ParsedAnnotations, ParsedAssertion, ParsedTemplate, ShapeField, ShapeKey,
    SymbolClaim, Trust, TypeContext, TypeId, TypeSyntax,
};

#[cfg(test)]
mod tests {
    #[test]
    fn the_api_version_starts_at_zero() {
        assert_eq!(super::PLUGIN_API_VERSION, 0);
    }
}
