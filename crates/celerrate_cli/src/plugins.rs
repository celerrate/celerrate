//! The composition root's plugin registration: the one place
//! implementations are constructed and set into the owning crates'
//! registries. Order here IS the deterministic dispatch order. The
//! registries sit in the high-durability tier next to stubs and
//! configuration, set once per process, never mutated.

use std::sync::Arc;

use celerrate_plugin::{PLUGIN_API_VERSION, PluginDescriptor};

use crate::database::AnalysisDatabase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedPlugin {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisteredPlugins {
    pub excluded: Vec<ExcludedPlugin>,
}

/// The dormant API-version gate (the parent's crash semantics:
/// exclude, degrade, never crash).
fn admission(descriptor: &PluginDescriptor) -> Result<(), String> {
    if descriptor.api_version == PLUGIN_API_VERSION {
        Ok(())
    } else {
        Err(format!(
            "plugin API version {} does not match the binary's {}",
            descriptor.api_version, PLUGIN_API_VERSION,
        ))
    }
}

pub fn register_plugins(database: &AnalysisDatabase) -> RegisteredPlugins {
    let mut excluded = Vec::new();
    let mut type_syntax = Vec::new();
    let mut virtual_symbols = Vec::new();
    let dynamic_providers = Vec::new();

    // Registration order, declared once: phpdoc-bridge first.
    let descriptor = celerrate_phpdoc_bridge::descriptor();
    match admission(&descriptor) {
        Ok(()) => {
            let bridge = Arc::new(celerrate_phpdoc_bridge::PhpdocBridge::new());
            type_syntax.push(celerrate_types::TypeSyntaxRegistration {
                identity: descriptor.identity.clone(),
                implementation: bridge.clone(),
            });
            virtual_symbols.push(celerrate_semantics::VirtualSymbolRegistration {
                identity: descriptor.identity,
                provider: bridge,
            });
        }
        Err(reason) => excluded.push(ExcludedPlugin {
            name: descriptor.identity.name,
            reason,
        }),
    }

    // Overlapping dynamic-provider claims exclude the later
    // registrant (no documented precedence exists yet) — dormant
    // until plan 7 registers the stdlib provider.
    if let Err(conflict) = celerrate_types::validate_claims(&dynamic_providers) {
        excluded.push(ExcludedPlugin {
            name: conflict.second.clone(),
            reason: format!(
                "claim conflict with {} on {:?}",
                conflict.first, conflict.claim
            ),
        });
        // The exclusion is recorded but the vector is NOT rebuilt —
        // with zero registered providers the branch is unreachable.
        // Plan 7, registering the first real provider, must rebuild
        // the vector without the excluded registrant and re-validate
        // before setting the registry.
    }

    let _ = celerrate_types::TypeSyntaxRegistry::builder(type_syntax)
        .durability(salsa::Durability::HIGH)
        .new(database);
    let _ = celerrate_semantics::VirtualSymbolRegistry::builder(virtual_symbols)
        .durability(salsa::Durability::HIGH)
        .new(database);
    let _ = celerrate_types::DynamicTypeProviderRegistry::builder(dynamic_providers)
        .durability(salsa::Durability::HIGH)
        .new(database);

    RegisteredPlugins { excluded }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use super::{admission, register_plugins};
    use crate::database::AnalysisDatabase;

    #[test]
    fn the_composition_root_registers_the_bridge_in_every_registry_it_serves() {
        let database = AnalysisDatabase::default();
        let plugins = register_plugins(&database);
        assert!(plugins.excluded.is_empty());
        let syntax = celerrate_types::TypeSyntaxRegistry::try_get(&database).unwrap();
        let registrations = syntax.registrations(&database);
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].identity.name, "phpdoc-bridge");
        assert_eq!(registrations[0].identity.version, env!("CARGO_PKG_VERSION"),);
        let virtual_symbols =
            celerrate_semantics::VirtualSymbolRegistry::try_get(&database).unwrap();
        assert_eq!(virtual_symbols.registrations(&database).len(), 1);
        let providers = celerrate_types::DynamicTypeProviderRegistry::try_get(&database).unwrap();
        assert!(providers.registrations(&database).is_empty());
    }

    #[test]
    fn an_api_version_mismatch_excludes_and_reports() {
        // Exercise the dormant check through the internal helper that
        // takes the descriptor as data (the public path cannot mismatch
        // for compiled-in plugins — the design says so; this pins the
        // scaffolding anyway).
        let mismatched = celerrate_plugin::PluginDescriptor {
            identity: celerrate_phpdoc_bridge::descriptor().identity,
            api_version: celerrate_plugin::PLUGIN_API_VERSION + 1,
        };
        let verdict = admission(&mismatched);
        assert!(matches!(verdict, Err(reason) if reason.contains("API version")));
    }
}
