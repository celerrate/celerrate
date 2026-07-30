//! The composition root's plugin registration: the one place
//! implementations are constructed and set into the owning crates'
//! registries. Order here IS the deterministic dispatch order. The
//! registries sit in the high-durability tier next to stubs and
//! configuration, set once per process, never mutated.

use std::collections::BTreeMap;
use std::sync::Arc;

use celerrate_plugin::{PLUGIN_API_VERSION, PluginDescriptor};
use celerrate_semantics::PluginIdentity;

use crate::database::AnalysisDatabase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedPlugin {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegisteredPlugins {
    /// The identities whose registrations actually entered a salsa
    /// registry, in registration order — dynamic providers counted
    /// after claim admission. This is the effective set the plugin-set
    /// digest keys the cache on (issue #60).
    pub admitted: Vec<PluginIdentity>,
    pub excluded: Vec<ExcludedPlugin>,
}

/// The dormant API-version gate (the parent's crash semantics:
/// exclude, degrade, never crash), plus the core-name reservation: the
/// `celerrate-core` identity is the composition root's own, keyed by
/// binary identity rather than the plugin-set digest, so a plugin
/// claiming it is excluded before the API-version check ever runs.
fn admission(descriptor: &PluginDescriptor) -> Result<(), String> {
    if descriptor.identity.name == celerrate_rules::CORE_IDENTITY_NAME {
        return Err(format!(
            "the name {} is reserved for core registrations",
            celerrate_rules::CORE_IDENTITY_NAME,
        ));
    }
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
    let mut admitted = Vec::new();
    let mut excluded = Vec::new();
    let mut type_syntax = Vec::new();
    let mut virtual_symbols = Vec::new();
    let mut comment_directives = Vec::new();
    let mut dynamic_providers = Vec::new();

    // The native directive provider: core, registered unconditionally,
    // under the reserved core identity, outside the admitted set, it
    // never keys the plugin-set digest; binary identity already keys
    // the cache for core behavior.
    comment_directives.push(celerrate_semantics::CommentDirectiveRegistration {
        identity: core_identity(),
        provider: Arc::new(celerrate_semantics::NativeDirectiveProvider),
    });

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
                identity: descriptor.identity.clone(),
                provider: bridge,
            });
            admitted.push(descriptor.identity);
        }
        Err(reason) => excluded.push(ExcludedPlugin {
            name: descriptor.identity.name,
            reason,
        }),
    }

    // Stdlib provider: the computation-dependent stdlib signatures
    // no declarative stub can express.
    let descriptor = celerrate_stdlib_provider::descriptor();
    match admission(&descriptor) {
        Ok(()) => {
            dynamic_providers.push(celerrate_types::DynamicTypeProviderRegistration {
                identity: descriptor.identity,
                provider: Arc::new(celerrate_stdlib_provider::StdlibProvider::new()),
            });
        }
        Err(reason) => excluded.push(ExcludedPlugin {
            name: descriptor.identity.name,
            reason,
        }),
    }

    // Overlapping dynamic-provider claims exclude the later
    // registrant: registration order above IS the precedence (first
    // claim wins), and the set is rebuilt and re-validated until it
    // is conflict-free (`admit_dynamic_providers` below). Only the
    // survivors of that rebuild are counted as admitted: a
    // claim-excluded provider never lands in `admitted`.
    let (dynamic_providers, rebuild_exclusions) = admit_dynamic_providers(dynamic_providers);
    excluded.extend(rebuild_exclusions);
    admitted.extend(
        dynamic_providers
            .iter()
            .map(|registration| registration.identity.clone()),
    );

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

    RegisteredPlugins { admitted, excluded }
}

/// Registers the core rules under the reserved core identity, outside
/// the admitted plugin set: core behavior is keyed by binary identity,
/// never by the plugin-set digest. Order here is
/// the deterministic dispatch order, like the other four registries.
///
/// Every composition root that composes diagnostics must call this
/// function. The `RuleRegistry` input has no default: if it is left
/// unset, `celerrate_rules` treats the registry as empty rather than
/// producing an error, so core-rule diagnostic families (for example
/// CEL0024) go silently missing, with no compile error to catch the
/// omission. `overrides` carries the configuration's `[rules]`
/// activation table (`configuration::rule_overrides`); an empty map
/// reproduces the tier-only default.
pub fn register_core_rules(database: &AnalysisDatabase, overrides: &BTreeMap<String, bool>) {
    let _ = celerrate_rules::RuleRegistry::builder(core_registrations(overrides))
        .durability(salsa::Durability::HIGH)
        .new(database);
}

/// The active-set formula: (`Default`-tier rules minus disabled)
/// union (nursery rules enabled). An override on the
/// tier's own default is a valid no-op, so promotions and demotions
/// never break existing configurations.
pub fn rule_is_active(tier: celerrate_rules::Tier, override_enabled: Option<bool>) -> bool {
    override_enabled.unwrap_or(tier == celerrate_rules::Tier::Default)
}

/// The core registrations under the configured active set. Split from
/// `register_core_rules` so the `--watch` reload can rebuild the same
/// list for the registry setter.
pub fn core_registrations(
    overrides: &BTreeMap<String, bool>,
) -> Vec<celerrate_rules::RuleRegistration> {
    let identity = core_identity();
    celerrate_rules::core_rules()
        .into_iter()
        .map(
            |(metadata, implementation)| celerrate_rules::RuleRegistration {
                identity: identity.clone(),
                active: rule_is_active(
                    metadata.tier,
                    overrides.get(metadata.name.as_str()).copied(),
                ),
                metadata,
                implementation,
            },
        )
        .collect()
}

fn core_identity() -> PluginIdentity {
    PluginIdentity {
        name: celerrate_rules::CORE_IDENTITY_NAME.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        configuration: String::new(),
    }
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

/// The plugin-set cache key (issue #60): a blake3 digest of the
/// **post-admission** effective set, the admitted identities'
/// `(name, version, configuration)` triples plus
/// the excluded plugin names. Derived from `register_plugins`' output,
/// so there is no second descriptor list to forget; sorted before
/// hashing, so registration order does not key the cache. Fields are
/// length-prefixed and sections count-prefixed straight into the
/// hasher: no serialization step, no failure arm.
pub fn plugin_set_digest(plugins: &RegisteredPlugins) -> [u8; 32] {
    let mut triples: Vec<(&str, &str, &str)> = plugins
        .admitted
        .iter()
        .map(|identity| {
            (
                identity.name.as_str(),
                identity.version.as_str(),
                identity.configuration.as_str(),
            )
        })
        .collect();
    triples.sort_unstable();
    let mut excluded: Vec<&str> = plugins
        .excluded
        .iter()
        .map(|plugin| plugin.name.as_str())
        .collect();
    excluded.sort_unstable();

    let mut hasher = blake3::Hasher::new();
    update_count(&mut hasher, triples.len());
    for (name, version, configuration) in triples {
        update_field(&mut hasher, name);
        update_field(&mut hasher, version);
        update_field(&mut hasher, configuration);
    }
    update_count(&mut hasher, excluded.len());
    for name in excluded {
        update_field(&mut hasher, name);
    }
    *hasher.finalize().as_bytes()
}

fn update_count(hasher: &mut blake3::Hasher, count: usize) {
    hasher.update(&(count as u64).to_le_bytes());
}

fn update_field(hasher: &mut blake3::Hasher, field: &str) {
    hasher.update(&(field.len() as u64).to_le_bytes());
    hasher.update(field.as_bytes());
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::{
        ExcludedPlugin, RegisteredPlugins, admission, core_registrations, plugin_set_digest,
        register_core_rules, register_plugins, rule_is_active,
    };
    use crate::database::AnalysisDatabase;

    #[test]
    fn the_active_computation_is_the_specs_formula() {
        use celerrate_rules::Tier;
        // (`Default`-tier rules minus disabled) union (nursery enabled).
        assert!(rule_is_active(Tier::Default, None));
        assert!(!rule_is_active(Tier::Default, Some(false)));
        assert!(rule_is_active(Tier::Default, Some(true)), "a valid no-op");
        assert!(!rule_is_active(Tier::Nursery, None));
        assert!(
            rule_is_active(Tier::Nursery, Some(true)),
            "force-activation"
        );
        assert!(!rule_is_active(Tier::Nursery, Some(false)), "a valid no-op");
    }

    #[test]
    fn an_override_reaches_the_registration_it_names_and_no_other() {
        let mut overrides = std::collections::BTreeMap::new();
        overrides.insert("null-dereference".to_owned(), false);
        let registrations = core_registrations(&overrides);
        for registration in &registrations {
            let expected = registration.metadata.name != "null-dereference";
            assert_eq!(
                registration.active, expected,
                "{}",
                registration.metadata.name
            );
        }
    }

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
        assert_eq!(comment_directive_registrations.len(), 2);
        assert_eq!(
            comment_directive_registrations[0].identity.name,
            "celerrate-core"
        );
        assert_eq!(
            comment_directive_registrations[1].identity.name,
            "phpdoc-bridge"
        );
        let providers = celerrate_types::DynamicTypeProviderRegistry::try_get(&database).unwrap();
        let provider_registrations = providers.registrations(&database);
        assert_eq!(provider_registrations.len(), 1);
        assert_eq!(provider_registrations[0].identity.name, "stdlib-provider");
    }

    #[test]
    fn the_native_directive_provider_never_enters_the_admitted_plugin_set() {
        let database = AnalysisDatabase::default();
        let plugins = register_plugins(&database);
        assert!(
            plugins
                .admitted
                .iter()
                .all(|identity| identity.name != celerrate_rules::CORE_IDENTITY_NAME)
        );
        let registry = celerrate_semantics::CommentDirectiveRegistry::try_get(&database).unwrap();
        assert_eq!(
            registry.registrations(&database)[0].identity.name,
            celerrate_rules::CORE_IDENTITY_NAME,
        );
    }

    #[test]
    fn an_api_version_mismatch_excludes_and_reports() {
        // Exercise the dormant check through the internal helper that
        // takes the descriptor as data (the public path cannot mismatch
        // for compiled-in plugins, this pins the scaffolding anyway).
        let mismatched = celerrate_plugin::PluginDescriptor::new(
            celerrate_phpdoc_bridge::descriptor().identity,
            celerrate_plugin::PLUGIN_API_VERSION + 1,
        );
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
        let admitted_names: Vec<&str> = admitted
            .iter()
            .map(|registration| registration.identity.name.as_str())
            .collect();
        assert!(
            !admitted_names.contains(&"second"),
            "a claim-excluded provider must never land in the admitted set",
        );
        assert!(
            !admitted_names.contains(&"third"),
            "a claim-excluded provider must never land in the admitted set",
        );
    }

    #[test]
    fn registration_records_the_admitted_identities_in_order() {
        let database = AnalysisDatabase::default();
        let plugins = register_plugins(&database);
        let bridge_name = celerrate_phpdoc_bridge::descriptor().identity.name;
        let stdlib_name = celerrate_stdlib_provider::descriptor().identity.name;
        assert_eq!(
            plugins
                .admitted
                .iter()
                .map(|identity| identity.name.as_str())
                .collect::<Vec<_>>(),
            vec![bridge_name.as_str(), stdlib_name.as_str()],
        );
        assert!(plugins.excluded.is_empty());
    }

    #[test]
    fn core_rules_register_under_the_reserved_identity_and_validate() {
        let db = AnalysisDatabase::default();
        register_core_rules(&db, &std::collections::BTreeMap::new());
        let registry =
            celerrate_rules::RuleRegistry::try_get(&db).expect("core rules are always registered");
        let registrations = registry.registrations(&db);
        assert!(!registrations.is_empty());
        assert!(registrations.iter().all(|registration| {
            registration.identity.name == celerrate_rules::CORE_IDENTITY_NAME
        }));
        assert_eq!(celerrate_rules::validate_rules(registrations), Ok(()));
    }

    #[test]
    fn core_rules_never_enter_the_admitted_plugin_set() {
        let db = AnalysisDatabase::default();
        let registered = register_plugins(&db);
        register_core_rules(&db, &std::collections::BTreeMap::new());
        assert!(
            registered
                .admitted
                .iter()
                .all(|identity| { identity.name != celerrate_rules::CORE_IDENTITY_NAME })
        );
    }

    #[test]
    fn a_plugin_claiming_the_reserved_core_name_is_excluded() {
        // Mirror `an_api_version_mismatch_excludes_and_reports`: build a
        // descriptor through the same fake-plugin path, but with the
        // reserved core identity name rather than a mismatched API
        // version. Admission must refuse it with a reason naming the
        // reservation, so the composition root would push it to
        // `excluded` and it never reaches a registry.
        let reserved = celerrate_plugin::PluginDescriptor::new(
            celerrate_semantics::PluginIdentity {
                name: celerrate_rules::CORE_IDENTITY_NAME.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                configuration: String::new(),
            },
            celerrate_plugin::PLUGIN_API_VERSION,
        );
        let verdict = admission(&reserved);
        let reason = verdict.expect_err("the reserved core name must be refused");
        assert!(
            reason.contains(celerrate_rules::CORE_IDENTITY_NAME),
            "the reason names the reserved identity: {reason}",
        );
        assert!(
            reason.contains("reserved"),
            "the reason states the reservation: {reason}",
        );
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
            _site: &celerrate_types::InvocationSite<'db, '_>,
        ) -> Option<celerrate_types::TypeId<'db>> {
            None
        }
    }

    /// A `PluginIdentity` with an empty configuration: the common case
    /// for the digest tests below.
    fn identity(name: &str, version: &str) -> celerrate_semantics::PluginIdentity {
        raw_identity(name, version, "")
    }

    /// A `PluginIdentity` with an explicit configuration, for the tests
    /// that need one.
    fn raw_identity(
        name: &str,
        version: &str,
        configuration: &str,
    ) -> celerrate_semantics::PluginIdentity {
        celerrate_semantics::PluginIdentity {
            name: name.to_owned(),
            version: version.to_owned(),
            configuration: configuration.to_owned(),
        }
    }

    #[test]
    fn an_exclusion_changes_the_digest() {
        let admitted = RegisteredPlugins {
            admitted: vec![identity("bridge", "1.0"), identity("provider", "1.0")],
            excluded: Vec::new(),
        };
        let degraded = RegisteredPlugins {
            admitted: vec![identity("bridge", "1.0")],
            excluded: vec![ExcludedPlugin {
                name: "provider".to_owned(),
                reason: "claim conflict".to_owned(),
            }],
        };
        assert_ne!(plugin_set_digest(&admitted), plugin_set_digest(&degraded));
    }

    #[test]
    fn the_exclusion_reason_wording_does_not_key_the_cache() {
        let one = RegisteredPlugins {
            admitted: Vec::new(),
            excluded: vec![ExcludedPlugin {
                name: "provider".to_owned(),
                reason: "old wording".to_owned(),
            }],
        };
        let other = RegisteredPlugins {
            excluded: vec![ExcludedPlugin {
                name: "provider".to_owned(),
                reason: "new wording".to_owned(),
            }],
            ..one.clone()
        };
        assert_eq!(plugin_set_digest(&one), plugin_set_digest(&other));
    }

    #[test]
    fn adjacent_fields_do_not_collide() {
        // Length prefixes: ("ab","c","") and ("a","bc","") must differ.
        let one = RegisteredPlugins {
            admitted: vec![raw_identity("ab", "c", "")],
            excluded: Vec::new(),
        };
        let other = RegisteredPlugins {
            admitted: vec![raw_identity("a", "bc", "")],
            excluded: Vec::new(),
        };
        assert_ne!(plugin_set_digest(&one), plugin_set_digest(&other));
    }

    #[test]
    fn the_plugin_set_digest_is_order_independent_and_identity_sensitive() {
        let alpha = identity("alpha", "1.0.0");
        let beta = raw_identity("beta", "2.0.0", "config");

        let forward = RegisteredPlugins {
            admitted: vec![alpha.clone(), beta.clone()],
            excluded: Vec::new(),
        };
        let reversed = RegisteredPlugins {
            admitted: vec![beta.clone(), alpha.clone()],
            excluded: Vec::new(),
        };
        assert_eq!(
            plugin_set_digest(&forward),
            plugin_set_digest(&reversed),
            "the same members in a different order digest equal: the digest sorts",
        );

        let renamed = raw_identity("gamma", "2.0.0", "config");
        let with_renamed = RegisteredPlugins {
            admitted: vec![alpha.clone(), renamed],
            excluded: Vec::new(),
        };
        assert_ne!(
            plugin_set_digest(&forward),
            plugin_set_digest(&with_renamed),
            "a changed name digests different",
        );

        let reversioned = raw_identity("beta", "3.0.0", "config");
        let with_reversioned = RegisteredPlugins {
            admitted: vec![alpha.clone(), reversioned],
            excluded: Vec::new(),
        };
        assert_ne!(
            plugin_set_digest(&forward),
            plugin_set_digest(&with_reversioned),
            "a changed version digests different",
        );

        let reconfigured = raw_identity("beta", "2.0.0", "other");
        let with_reconfigured = RegisteredPlugins {
            admitted: vec![alpha, reconfigured],
            excluded: Vec::new(),
        };
        assert_ne!(
            plugin_set_digest(&forward),
            plugin_set_digest(&with_reconfigured),
            "a changed configuration digests different",
        );
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
