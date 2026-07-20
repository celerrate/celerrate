//! The first migrated family (design section 5): the walk lives below
//! as `celerrate_semantics::gated_syntax_uses`; this rule consumes the
//! outcomes and constructs the diagnostics.

use celerrate_semantics::SYNTAX_NOT_AVAILABLE;

use crate::context::SyntaxContext;
use crate::finding::{FindingAnchor, FindingSink};
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::SyntaxRule;

/// A syntax construct newer than the range minimum.
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
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::PluginIdentity;
    use celerrate_source::FileId;

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

    #[test]
    fn the_rule_reproduces_the_legacy_query_byte_for_byte() {
        let source = "<?php\nreadonly class Point {}\nclass Box { public const int X = 1; }\n";
        let (db, file, configuration) = registered_setup(source, PhpVersion::new(8, 1));
        assert_eq!(
            syntax_phase_diagnostics(&db, file, configuration),
            celerrate_semantics::syntax_version_diagnostics(&db, file, configuration),
        );
        assert_eq!(syntax_phase_diagnostics(&db, file, configuration).len(), 2);
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
