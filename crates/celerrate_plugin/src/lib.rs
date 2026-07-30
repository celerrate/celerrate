//! The plugin API facade. Extension points are owned by their
//! consuming layers (`celerrate_types`, `celerrate_semantics`); this
//! crate aggregates and re-exports the stable surface so a plugin
//! crate declares exactly one dependency. Implementations are
//! constructed and registered at the composition root
//! (`celerrate_cli`). An extension point that proves insufficient is
//! extended, never bypassed.
//!
//! The API is deliberately not called v1: its second *dissimilar*
//! consumer, a framework dynamic type provider, is still to come.

/// The API version the composition root checks at registration. A
/// mismatch excludes the plugin for the whole run and the run is
/// reported degraded. For compiled-in first-party plugins the check
/// cannot fail — dormant scaffolding whose first real exercise is
/// the WASM host. Distinct from the plugin version inside
/// `PluginIdentity`; only the latter keys the cache.
pub const PLUGIN_API_VERSION: u32 = 0;

/// What a plugin exposes for registration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginDescriptor {
    pub identity: PluginIdentity,
    pub api_version: u32,
}

impl PluginDescriptor {
    /// Constructor for plugin crates and test suites: cross-crate
    /// literal construction is closed by `#[non_exhaustive]`.
    pub fn new(identity: PluginIdentity, api_version: u32) -> Self {
        Self {
            identity,
            api_version,
        }
    }
}

// Identity and the comment-directive and virtual-symbol extension points.
pub use celerrate_semantics::{
    CommentDirective, CommentDirectiveProvider, CommentKind, DirectiveOrigin, DirectiveScope,
    PluginIdentity, SuppressionIdentifier, VirtualMember, VirtualMemberKind, VirtualParameter,
    VirtualSymbolProvider,
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

// The rule-authoring surface: the traits a plugin implements, the
// sealed contexts they receive, and the metadata and reporting
// vocabulary around them. Nominal re-exports only — never the rule
// registry, never the core-only `Reporting` phase, never the
// construction seams (composition-root vocabulary: plugin-registered
// rules are deferred).
pub use celerrate_rules::{
    DiagnosticId, ExplainPage, FindingAnchor, FindingSink, RuleGroup, RuleIdentifier, RuleMetadata,
    SemanticRule, Severity, SyntaxContext, SyntaxRule, Tier, TypedBodyRule,
};
pub use celerrate_semantics::SemanticContext;
pub use celerrate_types::TypedBodyContext;

#[cfg(test)]
mod tests {
    #[test]
    fn the_api_version_starts_at_zero() {
        assert_eq!(super::PLUGIN_API_VERSION, 0);
    }
}
