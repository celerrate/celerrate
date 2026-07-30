//! The first migrated family: the walk lives below
//! as `celerrate_semantics::gated_syntax_uses`; this rule consumes the
//! outcomes and constructs the diagnostics.

use celerrate_diagnostics::DiagnosticId;

use crate::context::SyntaxContext;
use crate::finding::{FindingAnchor, FindingSink};
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::SyntaxRule;

/// A syntax construct newer than the range minimum.
pub const SYNTAX_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0024");

/// The rule that reports uses of syntax constructs newer than the
/// project's minimum PHP version.
pub struct SyntaxVersionGating;

/// The family's declarative unit.
pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "syntax-version-gating".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![RuleIdentifier {
            id: SYNTAX_NOT_AVAILABLE,
            severity: celerrate_diagnostics::Severity::Error,
        }],
        tier: Tier::Default,
    }
}

impl SyntaxRule for SyntaxVersionGating {
    fn check(&self, context: &SyntaxContext<'_>, sink: &mut FindingSink<'_>) {
        let minimum = context.php_version_range().minimum;
        for gated in context.gated_syntax_uses() {
            if gated.required > minimum {
                sink.report(
                    SYNTAX_NOT_AVAILABLE,
                    FindingAnchor::Range(gated.range),
                    format!(
                        "`{}` requires PHP {}, but the project's minimum PHP version is {minimum}",
                        gated.label, gated.required,
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_diagnostics::{Diagnostic, Severity};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::PluginIdentity;
    use celerrate_source::FileId;

    use super::SYNTAX_NOT_AVAILABLE;
    use crate::metadata::Tier;
    use crate::phases::syntax_phase_diagnostics;
    use crate::registry::{RuleRegistration, RuleRegistry};

    fn registered_setup(
        source: &str,
        minimum: PhpVersion,
    ) -> (TestDatabase, SourceFile, ProjectConfiguration) {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        let configuration =
            ProjectConfiguration::builder(PhpVersionRange::new(minimum, PhpVersion::new(8, 5)))
                .durability(salsa::Durability::MEDIUM)
                .new(&db);
        let identity = PluginIdentity {
            name: crate::CORE_IDENTITY_NAME.to_owned(),
            version: "test".to_owned(),
            configuration: String::new(),
        };
        let registrations = crate::rules::core_rules()
            .into_iter()
            .map(|(metadata, implementation)| RuleRegistration {
                identity: identity.clone(),
                active: metadata.tier == Tier::Default,
                metadata,
                implementation,
            })
            .collect();
        let _ = RuleRegistry::builder(registrations)
            .durability(salsa::Durability::HIGH)
            .new(&db);
        (db, file, configuration)
    }

    /// The full framework path for one source at one range minimum: the
    /// registry is populated from `core_rules`, so this exercises exactly
    /// what the CLI composes.
    fn gated(source: &str, minimum: PhpVersion) -> Vec<Diagnostic> {
        let (db, file, configuration) = registered_setup(source, minimum);
        syntax_phase_diagnostics(&db, file, configuration).clone()
    }

    #[test]
    fn a_readonly_class_is_gated_below_its_version() {
        let diagnostics = gated("<?php readonly class Point {}", PhpVersion::new(8, 1));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, SYNTAX_NOT_AVAILABLE);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(
            diagnostics[0].message,
            "`readonly class` requires PHP 8.2, but the project's minimum PHP version is 8.1",
        );
    }

    #[test]
    fn a_readonly_property_is_not_a_readonly_class() {
        assert_eq!(
            gated(
                "<?php class Point { public readonly int $x; }",
                PhpVersion::new(8, 1),
            ),
            vec![],
        );
    }

    #[test]
    fn each_gated_construct_reports_its_version() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "<?php function f((Left&Right)|null $x) {}",
                "parenthesized (DNF) type",
                "8.2",
            ),
            (
                "<?php class C { const int LIMIT = 1; }",
                "typed constant",
                "8.3",
            ),
            (
                "<?php $x = Config::{$name};",
                "dynamic class constant fetch",
                "8.3",
            ),
            (
                "<?php class C { public string $p { get => 'v'; } }",
                "property hooks",
                "8.4",
            ),
            (
                "<?php class C { public private(set) string $p; }",
                "asymmetric visibility",
                "8.4",
            ),
            ("<?php $y = $x |> strlen(...);", "pipe operator", "8.5"),
            (
                "<?php $c = clone($point, ['x' => 1]);",
                "clone with arguments",
                "8.5",
            ),
        ];
        for (source, label, version) in cases {
            let diagnostics = gated(source, PhpVersion::new(8, 1));
            let expected = format!(
                "`{label}` requires PHP {version}, but the project's minimum PHP version is 8.1",
            );
            assert!(
                diagnostics.iter().any(|d| d.message == expected),
                "{source}: {diagnostics:?}",
            );
            assert_eq!(gated(source, PhpVersion::new(8, 5)), vec![], "{source}");
        }
    }

    #[test]
    fn a_static_property_access_is_not_a_dynamic_constant_fetch() {
        assert_eq!(
            gated("<?php $x = Config::$value;", PhpVersion::new(8, 1)),
            vec![]
        );
    }

    #[test]
    fn a_single_positional_clone_argument_is_not_gated() {
        // Pre-8.5 PHP reads `clone($x)` as `clone` of a parenthesized
        // expression; gating it would be a false positive.
        assert_eq!(
            gated("<?php $c = clone($point);", PhpVersion::new(8, 1)),
            vec![]
        );
        let named = gated("<?php $c = clone(object: $point);", PhpVersion::new(8, 1));
        assert_eq!(named.len(), 1);
    }

    #[test]
    fn an_identifier_class_constant_is_not_a_dynamic_fetch() {
        assert_eq!(
            gated("<?php $x = Config::VERSION;", PhpVersion::new(8, 1)),
            vec![]
        );
    }

    #[test]
    fn a_promoted_property_with_asymmetric_visibility_is_gated() {
        let source =
            "<?php class C { public function __construct(public private(set) string $x) {} }";
        let diagnostics = gated(source, PhpVersion::new(8, 1));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "`asymmetric visibility` requires PHP 8.4, but the project's minimum PHP version is 8.1",
        );
        assert_eq!(gated(source, PhpVersion::new(8, 5)), vec![]);
    }

    #[test]
    fn a_construct_within_the_range_minimum_is_silent() {
        let (db, file, configuration) =
            registered_setup("<?php readonly class Point {}", PhpVersion::new(8, 2));
        assert!(syntax_phase_diagnostics(&db, file, configuration).is_empty());
    }

    #[test]
    fn the_message_names_the_construct_and_both_versions() {
        let (db, file, configuration) =
            registered_setup("<?php readonly class Point {}", PhpVersion::new(8, 1));
        let diagnostics = syntax_phase_diagnostics(&db, file, configuration);
        assert_eq!(
            diagnostics[0].message,
            "`readonly class` requires PHP 8.2, but the project's minimum PHP version is 8.1",
        );
    }
}
