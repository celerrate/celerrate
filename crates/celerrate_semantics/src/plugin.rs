//! Plugin identity: the vocabulary shared by every extension-point
//! registry. Defined here because `celerrate_semantics` is the lowest
//! crate that owns a registry; `celerrate_types` reuses it and
//! `celerrate_plugin` re-exports it.

/// The identity of one registered plugin. It travels in the same
/// salsa input as the implementation it identifies, so every read of
/// the implementation records a dependency on the identity too: a
/// version bump or a reconfiguration invalidates exactly like an
/// implementation change would, and the persistent cache's plugin-set
/// key (plan 9a) reads the same fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginIdentity {
    /// The public plugin name (for the bridge: `phpdoc-bridge`).
    pub name: String,
    /// The plugin's own version, distinct from the API version the
    /// composition root checks at registration.
    pub version: String,
    /// The plugin's configuration, serialized deterministically by
    /// the composition root. Part of the identity: a reconfigured
    /// plugin is a different plugin as far as invalidation goes.
    pub configuration: String,
}
