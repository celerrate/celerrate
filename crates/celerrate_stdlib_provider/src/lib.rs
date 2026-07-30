//! The stdlib type provider: a first-party
//! plugin computing the computation-dependent stdlib signatures no
//! declarative stub can express — the declarative long tail lives
//! in the refinements overlay instead. Stateless and pure: every
//! answer is a function of the `InvocationSite` alone (argument values
//! travel as literal types); `None` falls through to the declared
//! tier. Claims are exact folded function keys. Depends only on
//! `celerrate_plugin` (enforced by `cargo xtask dependency-shape`).

mod array_functions;
mod json_functions;
mod pattern_functions;
mod string_functions;

use celerrate_plugin::{DynamicTypeProvider, InvocationSite, SymbolClaim, TypeContext, TypeId};

/// Sorted; `claims()` maps it verbatim. Grown by tasks 7–9 and
/// curation, never speculatively.
const CLAIMED_FUNCTIONS: &[&str] = &[
    "array_filter",
    "array_map",
    "current",
    "end",
    "explode",
    "json_decode",
    "preg_match",
    "reset",
];

#[derive(Debug, Clone, Copy, Default)]
pub struct StdlibProvider;

impl StdlibProvider {
    pub fn new() -> Self {
        Self
    }
}

pub fn descriptor() -> celerrate_plugin::PluginDescriptor {
    celerrate_plugin::PluginDescriptor::new(
        celerrate_plugin::PluginIdentity {
            name: "stdlib-provider".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            configuration: String::new(),
        },
        celerrate_plugin::PLUGIN_API_VERSION,
    )
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

    fn return_type<'db>(&self, site: &InvocationSite<'db, '_>) -> Option<TypeId<'db>> {
        let SymbolClaim::Function { key } = site.claim() else {
            return None;
        };
        function_return(site.types(), key, site.argument_types())
    }

    fn by_reference_types<'db>(&self, site: &InvocationSite<'db, '_>) -> Vec<(usize, TypeId<'db>)> {
        let SymbolClaim::Function { key } = site.claim() else {
            return Vec::new();
        };
        match key.as_str() {
            "preg_match" => {
                pattern_functions::preg_match_matches(site.types(), site.argument_types())
                    .map(|matches| vec![(2, matches)])
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }
}

fn function_return<'db>(
    context: TypeContext<'db>,
    key: &str,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    match key {
        "array_filter" => array_functions::array_filter(context, arguments),
        "array_map" => array_functions::array_map(context, arguments),
        "current" | "end" | "reset" => array_functions::pointer_value(context, arguments),
        "explode" => string_functions::explode(context, arguments),
        "json_decode" => json_functions::json_decode(context, arguments),
        "preg_match" => Some(pattern_functions::preg_match_return(context)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::{DynamicTypeProvider, SymbolClaim, TypeId};
    use celerrate_types::{Invocation, testing_invocation_site};

    use super::StdlibProvider;

    pub(crate) fn function_invocation<'db>(
        key: &str,
        arguments: Vec<TypeId<'db>>,
    ) -> Invocation<'db> {
        Invocation::new(
            SymbolClaim::Function {
                key: key.to_owned(),
            },
            None,
            arguments,
        )
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
        let invocation = function_invocation("current", vec![subject]);
        let answer = provider
            .return_type(&testing_invocation_site(&db, &invocation))
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
        let invocation = function_invocation("current", vec![subject]);
        let answer = provider
            .return_type(&testing_invocation_site(&db, &invocation))
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
        let unknown_subject = function_invocation("current", vec![TypeId::mixed(&db)]);
        assert!(
            provider
                .return_type(&testing_invocation_site(&db, &unknown_subject))
                .is_none(),
        );
        let no_arguments = function_invocation("current", vec![]);
        assert!(
            provider
                .return_type(&testing_invocation_site(&db, &no_arguments))
                .is_none(),
        );
    }

    /// The adjudicated resolution: an empty shape (the literal `[]`)
    /// is NOT an unknown subject.
    /// `array_value` already answers `never` for it (the union of
    /// zero field values), so `current([])` answers the concrete
    /// `false` literal — matching real PHP semantics for `current`
    /// on an empty array — rather than falling through to `None`.
    #[test]
    fn current_over_an_empty_shape_answers_the_false_literal() {
        let db = TestDatabase::default();
        let provider = StdlibProvider::new();
        let subject = TypeId::shape(&db, vec![]);
        let invocation = function_invocation("current", vec![subject]);
        let answer = provider
            .return_type(&testing_invocation_site(&db, &invocation))
            .unwrap();
        assert_eq!(answer, TypeId::bool_literal(&db, false));
    }

    /// Totality gate over `CLAIMED_FUNCTIONS`, mirroring `celerrate_types`'s
    /// `every_embedded_refinement_text_lowers`: every key the
    /// provider claims must actually answer `Some` through
    /// `function_return`. Without this, a typo in a match arm would
    /// claim a key and then silently answer `None` — the provider
    /// would swallow the call and fall through to the declared tier
    /// with nothing failing.
    ///
    /// This gate covers `return_type`'s dispatch only. The
    /// by-reference channel (`by_reference_types`, above) has no
    /// analogous totality check: today it has exactly one claimant
    /// (`preg_match`), covered end to end by
    /// `celerrate_types/tests/by_reference.rs` instead, so the gap has
    /// no live symptom. If a second `by_reference_types` contributor
    /// is ever added, a by-reference-specific totality check (a typo'd
    /// match arm silently contributing nothing) is owed alongside it.
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
            let invocation = function_invocation(key, arguments);
            let answer = provider.return_type(&testing_invocation_site(&db, &invocation));
            assert!(
                answer.is_some(),
                "claimed function {key:?} answered None: it is claimed but \
                 has no working dispatch arm in `function_return`",
            );
        }
    }

    /// The subject fed to each claimed function by
    /// [`every_claimed_function_answers_some_for_its_subject`]. The
    /// trailing `_ => None` arm is a deliberate forcing function, not
    /// a shortcut: the caller asserts `subject.is_some()` with an
    /// actionable message, so any claimed function without a matching
    /// arm here fails that assertion by name instead of silently
    /// skipping the check. A future claim needing a different subject
    /// shape (a `preg_match` pattern, say) still needs its own arm:
    /// falling through to the wildcard only earns a named test
    /// failure, not coverage.
    fn claimed_function_subject<'db>(db: &'db TestDatabase, key: &str) -> Option<Vec<TypeId<'db>>> {
        match key {
            "array_filter" => Some(vec![TypeId::array(db, TypeId::int(db), TypeId::string(db))]),
            "array_map" => Some(vec![
                TypeId::callable(db, vec![], TypeId::int(db)),
                TypeId::array(db, TypeId::int(db), TypeId::string(db)),
            ]),
            "current" | "end" | "reset" => {
                Some(vec![TypeId::array(db, TypeId::int(db), TypeId::string(db))])
            }
            // A negative literal limit, so the dispatch reaches the
            // `list<string>` branch rather than the trivially-`Some`
            // default (no-limit) path.
            "explode" => Some(vec![
                TypeId::string(db),
                TypeId::string(db),
                TypeId::int_literal(db, -1),
            ]),
            // A `false` associative literal alongside a flags argument
            // that carries `JSON_OBJECT_AS_ARRAY`: PHP's BC-reasons
            // override means the literal wins over the flag (decision
            // 12, amended), so the dispatch reaches that
            // associative-overrides-flags path rather than the
            // trivially-`Some` no-arguments default.
            "json_decode" => Some(vec![
                TypeId::string(db),
                TypeId::bool_literal(db, false),
                TypeId::int_literal(db, 512),
                TypeId::int_literal(db, crate::json_functions::JSON_OBJECT_AS_ARRAY),
            ]),
            // `preg_match`'s return type (`0|1|false`) never branches on
            // its arguments, so this subject exercises the handler's
            // only path — a realistic call, pattern and subject, rather
            // than an empty argument list. The pattern-derived
            // `$matches` shape is a separate channel
            // (`by_reference_types`), covered by `pattern_functions.rs`'s
            // own tests and by `celerrate_types/tests/by_reference.rs`'s
            // end-to-end fixture, since this totality gate only
            // exercises `return_type`.
            "preg_match" => Some(vec![
                TypeId::string_literal(db, "/(?<year>\\d+)/"),
                TypeId::string(db),
            ]),
            _ => None,
        }
    }
}
