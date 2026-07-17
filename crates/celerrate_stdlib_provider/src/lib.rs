//! The stdlib type provider (design section 7): a first-party
//! plugin computing the computation-dependent stdlib signatures no
//! declarative stub can express — the declarative long tail lives
//! in the refinements overlay instead. Stateless and pure: every
//! answer is a function of the `Invocation` alone (argument values
//! travel as literal types); `None` falls through to the declared
//! tier. Claims are exact folded function keys. Depends only on
//! `celerrate_plugin` (enforced by `cargo xtask dependency-shape`).

mod array_functions;

use celerrate_plugin::{DynamicTypeProvider, Invocation, SymbolClaim, TypeId, salsa};

/// Sorted; `claims()` maps it verbatim. Grown by tasks 7–9 and
/// curation, never speculatively.
const CLAIMED_FUNCTIONS: &[&str] = &["current", "end", "reset"];

#[derive(Debug, Clone, Copy, Default)]
pub struct StdlibProvider;

impl StdlibProvider {
    pub fn new() -> Self {
        Self
    }
}

pub fn descriptor() -> celerrate_plugin::PluginDescriptor {
    celerrate_plugin::PluginDescriptor {
        identity: celerrate_plugin::PluginIdentity {
            name: "stdlib-provider".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            configuration: String::new(),
        },
        api_version: celerrate_plugin::PLUGIN_API_VERSION,
    }
}

impl DynamicTypeProvider for StdlibProvider {
    fn claims(&self) -> Vec<SymbolClaim> {
        CLAIMED_FUNCTIONS
            .iter()
            .map(|key| SymbolClaim::Function {
                key: (*key).to_owned(),
            })
            .collect()
    }

    fn return_type<'db>(
        &self,
        db: &'db dyn salsa::Database,
        invocation: &Invocation<'db>,
    ) -> Option<TypeId<'db>> {
        let SymbolClaim::Function { key } = &invocation.claim else {
            return None;
        };
        function_return(db, key, &invocation.argument_types)
    }
}

fn function_return<'db>(
    db: &'db dyn salsa::Database,
    key: &str,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    match key {
        "current" | "end" | "reset" => array_functions::pointer_value(db, arguments),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::{DynamicTypeProvider, Invocation, SymbolClaim, TypeId};

    use super::StdlibProvider;

    pub(crate) fn function_invocation<'db>(
        key: &str,
        arguments: Vec<TypeId<'db>>,
    ) -> Invocation<'db> {
        Invocation {
            claim: SymbolClaim::Function {
                key: key.to_owned(),
            },
            receiver_type: None,
            argument_types: arguments,
        }
    }

    #[test]
    fn the_descriptor_names_the_plugin_and_the_api_version() {
        let descriptor = super::descriptor();
        assert_eq!(descriptor.identity.name, "stdlib-provider");
        assert_eq!(descriptor.api_version, celerrate_plugin::PLUGIN_API_VERSION);
    }

    #[test]
    fn claims_are_sorted_and_distinct() {
        let mut sorted = super::CLAIMED_FUNCTIONS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.as_slice(), super::CLAIMED_FUNCTIONS);
    }

    #[test]
    fn current_projects_the_value_type_with_the_false_miss() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        let subject = TypeId::array(&db, TypeId::int(&db), TypeId::string(&db));
        let answer = provider
            .return_type(&db, &function_invocation("current", vec![subject]))
            .unwrap();
        assert_eq!(
            answer,
            TypeId::union(&db, [TypeId::string(&db), TypeId::bool_literal(&db, false)],),
        );
    }

    #[test]
    fn current_over_a_shape_unions_the_field_values() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        let subject = TypeId::shape(
            &db,
            vec![
                celerrate_plugin::ShapeField {
                    key: celerrate_plugin::ShapeKey::String("a".to_owned()),
                    optional: false,
                    value: TypeId::int_literal(&db, 1),
                },
                celerrate_plugin::ShapeField {
                    key: celerrate_plugin::ShapeKey::String("b".to_owned()),
                    optional: false,
                    value: TypeId::string_literal(&db, "x"),
                },
            ],
        );
        let answer = provider
            .return_type(&db, &function_invocation("current", vec![subject]))
            .unwrap();
        assert_eq!(
            answer,
            TypeId::union(
                &db,
                [
                    TypeId::int_literal(&db, 1),
                    TypeId::string_literal(&db, "x"),
                    TypeId::bool_literal(&db, false),
                ],
            ),
        );
    }

    #[test]
    fn an_unknown_subject_answers_none_and_falls_through() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        assert!(
            provider
                .return_type(
                    &db,
                    &function_invocation("current", vec![TypeId::mixed(&db)]),
                )
                .is_none(),
        );
        assert!(
            provider
                .return_type(&db, &function_invocation("current", vec![]))
                .is_none(),
        );
    }

    /// The adjudicated resolution (tasks 6/7 defect, closed): an
    /// empty shape (the literal `[]`) is NOT an unknown subject.
    /// `array_value` already answers `never` for it (the union of
    /// zero field values), so `current([])` answers the concrete
    /// `false` literal — matching real PHP semantics for `current`
    /// on an empty array — rather than falling through to `None`.
    #[test]
    fn current_over_an_empty_shape_answers_the_false_literal() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        let subject = TypeId::shape(&db, vec![]);
        let answer = provider
            .return_type(&db, &function_invocation("current", vec![subject]))
            .unwrap();
        assert_eq!(answer, TypeId::bool_literal(&db, false));
    }

    /// Totality gate over `CLAIMED_FUNCTIONS` (task 6 review, Minor
    /// 2), mirroring `celerrate_types`'s
    /// `every_embedded_refinement_text_lowers`: every key the
    /// provider claims must actually answer `Some` through
    /// `function_return`. Without this, a typo in a match arm would
    /// claim a key and then silently answer `None` — the provider
    /// would swallow the call and fall through to the declared tier
    /// with nothing failing.
    #[test]
    fn every_claimed_function_answers_some_for_its_subject() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        for key in super::CLAIMED_FUNCTIONS {
            let subject = claimed_function_subject(&db, key);
            assert!(
                subject.is_some(),
                "no subject configured for claimed function {key:?} in \
                 `claimed_function_subject`; growing CLAIMED_FUNCTIONS \
                 requires adding a matching subject here so this totality \
                 test keeps verifying the new dispatch arm",
            );
            let arguments = subject.unwrap_or_default();
            let answer = provider.return_type(&db, &function_invocation(key, arguments));
            assert!(
                answer.is_some(),
                "claimed function {key:?} answered None: it is claimed but \
                 has no working dispatch arm in `function_return`",
            );
        }
    }

    /// The subject fed to each claimed function by
    /// [`every_claimed_function_answers_some_for_its_subject`].
    /// Deliberately has no wildcard arm: a claimed function with no
    /// arm here answers `None`, which the caller turns into a named
    /// assertion failure rather than silently skipping the check. A
    /// future claim needing a different subject shape (a `preg_match`
    /// pattern, say) gets its own arm instead of reusing this one.
    fn claimed_function_subject<'db>(db: &'db TestDatabase, key: &str) -> Option<Vec<TypeId<'db>>> {
        match key {
            "current" | "end" | "reset" => {
                Some(vec![TypeId::array(db, TypeId::int(db), TypeId::string(db))])
            }
            _ => None,
        }
    }
}
