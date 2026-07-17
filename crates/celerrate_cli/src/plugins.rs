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
    let mut comment_directives = Vec::new();
    let mut dynamic_providers = Vec::new();

    // Registration order, declared once: phpdoc-bridge first.
    let descriptor = celerrate_phpdoc_bridge::descriptor();
    match admission(&descriptor) {
        Ok(()) => {
            let bridge = Arc::new(celerrate_phpdoc_bridge::PhpdocBridge::new());
            type_syntax.push(celerrate_types::TypeSyntaxRegistration {
                identity: descriptor.identity.clone(),
                implementation: bridge.clone(),
            });
            comment_directives.push(celerrate_semantics::CommentDirectiveRegistration {
                identity: descriptor.identity.clone(),
                provider: bridge.clone(),
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

    // Stdlib provider: the computation-dependent stdlib signatures
    // no declarative stub can express.
    match admission(&celerrate_stdlib_provider::descriptor()) {
        Ok(()) => {
            dynamic_providers.push(celerrate_types::DynamicTypeProviderRegistration {
                identity: celerrate_stdlib_provider::descriptor().identity,
                provider: Arc::new(celerrate_stdlib_provider::StdlibProvider::new()),
            });
        }
        Err(reason) => excluded.push(ExcludedPlugin {
            name: celerrate_stdlib_provider::descriptor().identity.name,
            reason,
        }),
    }

    // Overlapping dynamic-provider claims exclude the later
    // registrant: registration order above IS the precedence (first
    // claim wins), and the set is rebuilt and re-validated until it
    // is conflict-free (`admit_dynamic_providers` below).
    let (dynamic_providers, rebuild_exclusions) = admit_dynamic_providers(dynamic_providers);
    excluded.extend(rebuild_exclusions);

    let _ = celerrate_types::TypeSyntaxRegistry::builder(type_syntax)
        .durability(salsa::Durability::HIGH)
        .new(database);
    let _ = celerrate_semantics::VirtualSymbolRegistry::builder(virtual_symbols)
        .durability(salsa::Durability::HIGH)
        .new(database);
    let _ = celerrate_semantics::CommentDirectiveRegistry::builder(comment_directives)
        .durability(salsa::Durability::HIGH)
        .new(database);
    let _ = celerrate_types::DynamicTypeProviderRegistry::builder(dynamic_providers)
        .durability(salsa::Durability::HIGH)
        .new(database);

    RegisteredPlugins { excluded }
}

/// Overlapping claims exclude the later registrant and the set is
/// rebuilt until it validates: registration order is the precedence.
fn admit_dynamic_providers(
    mut registrations: Vec<celerrate_types::DynamicTypeProviderRegistration>,
) -> (
    Vec<celerrate_types::DynamicTypeProviderRegistration>,
    Vec<ExcludedPlugin>,
) {
    let mut excluded = Vec::new();
    while let Err(conflict) = celerrate_types::validate_claims(&registrations) {
        excluded.push(ExcludedPlugin {
            name: conflict.second.clone(),
            reason: format!(
                "claim conflict with {} on {:?}",
                conflict.first, conflict.claim,
            ),
        });
        registrations.retain(|registration| registration.identity.name != conflict.second);
    }
    (registrations, excluded)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

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
        let comment_directives =
            celerrate_semantics::CommentDirectiveRegistry::try_get(&database).unwrap();
        let comment_directive_registrations = comment_directives.registrations(&database);
        assert_eq!(comment_directive_registrations.len(), 1);
        assert_eq!(
            comment_directive_registrations[0].identity.name,
            "phpdoc-bridge"
        );
        let providers = celerrate_types::DynamicTypeProviderRegistry::try_get(&database).unwrap();
        let provider_registrations = providers.registrations(&database);
        assert_eq!(provider_registrations.len(), 1);
        assert_eq!(provider_registrations[0].identity.name, "stdlib-provider");
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

    #[test]
    fn the_stdlib_provider_registers_with_its_claims() {
        let database = AnalysisDatabase::default();
        let registered = register_plugins(&database);
        assert!(registered.excluded.is_empty());
        let registry =
            celerrate_types::DynamicTypeProviderRegistry::try_get(&database).expect("set");
        let registrations = registry.registrations(&database);
        let provider = registrations
            .iter()
            .find(|registration| registration.identity.name == "stdlib-provider")
            .expect("registered");
        assert!(!provider.provider.claims().is_empty());
    }

    #[test]
    fn a_claim_conflict_excludes_the_later_registrant_and_rebuilds() {
        // Unit-level: three mutually conflicting registrations (all
        // claiming the same function). Two registrants would only
        // ever exercise a single pass, which a non-rebuilding shape
        // (`if let Err` + one `retain`) would pass identically. The
        // third registrant forces a second loop iteration: after the
        // first pass excludes "second", the set of [first, third]
        // still conflicts, so only the `while` loop — not a single
        // `if` — drives it to a conflict-free result.
        let (admitted, excluded) = super::admit_dynamic_providers(vec![
            fake_registration("first", &["current"]),
            fake_registration("second", &["current"]),
            fake_registration("third", &["current"]),
        ]);
        assert_eq!(admitted.len(), 1);
        assert_eq!(excluded.len(), 2);
        assert_eq!(excluded.first().unwrap().name, "second");
        assert_eq!(excluded.get(1).unwrap().name, "third");
        assert!(celerrate_types::validate_claims(&admitted).is_ok());
    }

    #[derive(Debug)]
    struct FakeProvider {
        claimed: Vec<celerrate_types::SymbolClaim>,
    }

    impl celerrate_types::DynamicTypeProvider for FakeProvider {
        fn claims(&self) -> Vec<celerrate_types::SymbolClaim> {
            self.claimed.clone()
        }

        fn return_type<'db>(
            &self,
            _db: &'db dyn salsa::Database,
            _invocation: &celerrate_types::Invocation<'db>,
        ) -> Option<celerrate_types::TypeId<'db>> {
            None
        }
    }

    fn fake_registration(
        name: &str,
        keys: &[&str],
    ) -> celerrate_types::DynamicTypeProviderRegistration {
        celerrate_types::DynamicTypeProviderRegistration {
            identity: celerrate_semantics::PluginIdentity {
                name: name.to_owned(),
                version: "0.0.0".to_owned(),
                configuration: String::new(),
            },
            provider: std::sync::Arc::new(FakeProvider {
                claimed: keys
                    .iter()
                    .map(|key| celerrate_types::SymbolClaim::Function {
                        key: (*key).to_owned(),
                    })
                    .collect(),
            }),
        }
    }
}
