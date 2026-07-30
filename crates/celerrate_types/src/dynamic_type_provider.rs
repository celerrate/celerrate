//! The dynamic-type-provider extension point: contribute return types
//! at call sites. Owned by this crate; the registry
//! input lives here too, or the DAG would break upward. Dispatch rule,
//! fixed now: providers claim symbols; overlapping claims are a
//! registration-time error unless resolved by documented precedence at
//! the composition root (none is documented yet — the composition root
//! excludes the later registrant and reports the run degraded).
//! Deterministic: claims are gathered in registered order. A second,
//! optional channel (`by_reference_types`) lets a provider refine the
//! type a by-reference argument holds after the call — `preg_match`'s
//! pattern-derived `$matches` shape is the first consumer — layered on
//! top of the declared write-back the flow walker already applies.

use std::sync::Arc;

use celerrate_semantics::PluginIdentity;

use crate::representation::TypeId;

/// A symbol within a resolved callable, claimed by a dynamic-type
/// provider. Folded keys, normalized for comparison — the same form as
/// the symbol table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SymbolClaim {
    /// Global function.
    Function { key: String },
    /// Method: class, then method (both folded keys).
    Method {
        class_key: String,
        method_key: String,
    },
}

/// The invocation context for a dynamic-type provider querying the
/// return type at a call site. Engine- and test-internal: plugins observe
/// an invocation only through `InvocationSite` (argument *values* travel as
/// literal types interrogable on `TypeId`).
#[non_exhaustive]
pub struct Invocation<'db> {
    pub claim: SymbolClaim,
    pub receiver_type: Option<TypeId<'db>>,
    pub argument_types: Vec<TypeId<'db>>,
}

impl<'db> Invocation<'db> {
    /// Constructor for the engine and for test suites: cross-crate
    /// literal construction is closed by `#[non_exhaustive]`.
    pub fn new(
        claim: SymbolClaim,
        receiver_type: Option<TypeId<'db>>,
        argument_types: Vec<TypeId<'db>>,
    ) -> Self {
        Self {
            claim,
            receiver_type,
            argument_types,
        }
    }
}

/// The call-scoped context a dynamic-type provider answers from: the
/// invocation plus the sealed type facade. Owns the database
/// privately — a provider can neither name nor obtain
/// `salsa::Database` (the WASM-projectable shape, sketch section 7).
/// Constructed only by the engine's consumption points.
pub struct InvocationSite<'db, 'call> {
    db: &'db dyn salsa::Database,
    invocation: &'call Invocation<'db>,
}

impl<'db, 'call> InvocationSite<'db, 'call> {
    pub(crate) fn new(db: &'db dyn salsa::Database, invocation: &'call Invocation<'db>) -> Self {
        Self { db, invocation }
    }

    pub fn claim(&self) -> &'call SymbolClaim {
        &self.invocation.claim
    }

    pub fn receiver_type(&self) -> Option<TypeId<'db>> {
        self.invocation.receiver_type
    }

    pub fn argument_types(&self) -> &'call [TypeId<'db>] {
        &self.invocation.argument_types
    }

    /// The sealed type facade. Call-scoped like the site itself.
    pub fn types(&self) -> crate::type_context::TypeContext<'db> {
        crate::type_context::TypeContext::new(self.db)
    }
}

/// Test-only construction seam, same contract as
/// `testing_type_context`.
pub fn testing_invocation_site<'db, 'call>(
    db: &'db dyn salsa::Database,
    invocation: &'call Invocation<'db>,
) -> InvocationSite<'db, 'call> {
    InvocationSite::new(db, invocation)
}

/// An implementation contributes return types at call sites for claimed
/// symbols. Contributions are widened at the consumption boundary inside
/// `celerrate_types` (the crate's own fixpoint) — a provider never
/// controls termination. Implementations must be deterministic and
/// monotone; `None` falls back to the declared or inferred type.
///
/// Contributions feed fixpoint iteration: a provider is expected to
/// answer monotonically with respect to its argument types (a wider
/// invocation never yields a strictly narrower answer). The
/// expectation is documented, not enforced — a non-convergent
/// contribution hits the iteration budget and the result widens to
/// `mixed`, the deterministic bailout: a plugin never controls
/// termination.
///
/// **The persisted-cache purity obligation.** A
/// provider's answer must be a pure function of its `InvocationSite`
/// and its `PluginIdentity`: the persistent cache records no
/// per-answer dependency, so a provider that
/// reads cross-file state would silently break warm revalidation —
/// extend the record vocabulary in `celerrate_types::records` before
/// shipping one.
pub trait DynamicTypeProvider: Send + Sync {
    /// All symbols this provider claims to handle. Used for
    /// overlap detection at registration time.
    fn claims(&self) -> Vec<SymbolClaim>;
    /// Return type for an invocation, if the provider wishes to
    /// contribute one.
    fn return_type<'db>(&self, site: &InvocationSite<'db, '_>) -> Option<TypeId<'db>>;
    /// By-reference parameter refinements for a claimed invocation:
    /// (positional parameter index, the type the argument holds after
    /// the call). The default contributes nothing. Same purity and
    /// monotonicity contract as `return_type`; contributions are
    /// widened at the consumption boundary. Positional only — the
    /// consumer skips labeled arguments and stops at a spread.
    fn by_reference_types<'db>(&self, site: &InvocationSite<'db, '_>) -> Vec<(usize, TypeId<'db>)> {
        let _ = site;
        Vec::new()
    }
}

/// One registration: the implementation travels with its identity,
/// so reading it records the dependency an upgrade invalidates.
pub struct DynamicTypeProviderRegistration {
    pub identity: PluginIdentity,
    pub provider: Arc<dyn DynamicTypeProvider>,
}

impl std::fmt::Debug for DynamicTypeProviderRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DynamicTypeProviderRegistration")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// Set once per process at the composition root, HIGH durability,
/// never mutated. Unset (every plain test database): the no-plugin
/// path — dynamic contributions answer the declared or inferred type.
#[salsa::input(singleton)]
pub struct DynamicTypeProviderRegistry {
    #[returns(ref)]
    pub registrations: Vec<DynamicTypeProviderRegistration>,
}

/// A claim conflict: two providers registered for the same symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimConflict {
    pub claim: SymbolClaim,
    pub first: String,  // plugin name holding the claim
    pub second: String, // plugin name colliding with it
}

/// Validate that all claims in the registry are disjoint. Overlaps are
/// a registration-time error, detected deterministically in registered
/// order.
pub fn validate_claims(
    registrations: &[DynamicTypeProviderRegistration],
) -> Result<(), ClaimConflict> {
    let mut holders: std::collections::BTreeMap<SymbolClaim, String> =
        std::collections::BTreeMap::new();
    for registration in registrations {
        for claim in registration.provider.claims() {
            if let Some(first) = holders.get(&claim) {
                return Err(ClaimConflict {
                    claim,
                    first: first.clone(),
                    second: registration.identity.name.clone(),
                });
            }
            holders.insert(claim, registration.identity.name.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_semantics::PluginIdentity;

    use super::{
        ClaimConflict, DynamicTypeProvider, DynamicTypeProviderRegistration, Invocation,
        InvocationSite, SymbolClaim, validate_claims,
    };
    use crate::representation::TypeId;

    #[derive(Debug)]
    struct FakeProvider {
        claimed: Vec<SymbolClaim>,
    }

    impl DynamicTypeProvider for FakeProvider {
        fn claims(&self) -> Vec<SymbolClaim> {
            self.claimed.clone()
        }
        fn return_type<'db>(&self, site: &InvocationSite<'db, '_>) -> Option<TypeId<'db>> {
            Some(site.types().int())
        }
    }

    fn identity(name: &str) -> PluginIdentity {
        PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    fn registration(name: &str, claimed: Vec<SymbolClaim>) -> DynamicTypeProviderRegistration {
        DynamicTypeProviderRegistration {
            identity: identity(name),
            provider: std::sync::Arc::new(FakeProvider { claimed }),
        }
    }

    #[test]
    fn disjoint_claims_validate() {
        let registrations = vec![
            registration(
                "first",
                vec![SymbolClaim::Function {
                    key: "array_map".to_owned(),
                }],
            ),
            registration(
                "second",
                vec![SymbolClaim::Function {
                    key: "explode".to_owned(),
                }],
            ),
        ];
        assert_eq!(validate_claims(&registrations), Ok(()));
    }

    #[test]
    fn overlapping_claims_are_a_registration_time_error_naming_both_plugins() {
        let claim = SymbolClaim::Method {
            class_key: "collection".to_owned(),
            method_key: "map".to_owned(),
        };
        let registrations = vec![
            registration("first", vec![claim.clone()]),
            registration("second", vec![claim.clone()]),
        ];
        assert_eq!(
            validate_claims(&registrations),
            Err(ClaimConflict {
                claim,
                first: "first".to_owned(),
                second: "second".to_owned(),
            }),
        );
    }

    #[test]
    fn a_provider_overlapping_itself_is_also_refused() {
        let claim = SymbolClaim::Function {
            key: "current".to_owned(),
        };
        let registrations = vec![registration("solo", vec![claim.clone(), claim.clone()])];
        assert!(validate_claims(&registrations).is_err());
    }

    #[test]
    fn the_by_reference_channel_defaults_to_empty() {
        let db = TestDatabase::default();
        let provider = FakeProvider { claimed: vec![] };
        let invocation = Invocation {
            claim: SymbolClaim::Function {
                key: "any".to_owned(),
            },
            receiver_type: None,
            argument_types: vec![],
        };
        assert!(
            provider
                .by_reference_types(&InvocationSite::new(&db, &invocation))
                .is_empty()
        );
    }

    #[test]
    fn the_invocation_site_exposes_the_invocation_and_the_sealed_facade() {
        let db = TestDatabase::default();
        let claim = SymbolClaim::Function {
            key: "array_map".to_owned(),
        };
        let invocation = Invocation {
            claim: claim.clone(),
            receiver_type: None,
            argument_types: vec![TypeId::int(&db)],
        };
        let site = InvocationSite::new(&db, &invocation);
        assert_eq!(site.claim(), &claim);
        assert_eq!(site.receiver_type(), None);
        assert_eq!(site.argument_types(), &[TypeId::int(&db)]);
        assert_eq!(site.types().int(), TypeId::int(&db));
    }
}
