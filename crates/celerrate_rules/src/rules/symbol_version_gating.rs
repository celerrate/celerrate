//! The symbol version-gating family, migrated (design section 5): the
//! resolution walk lives below as
//! `celerrate_semantics::reference_outcomes`; this rule judges each
//! stub outcome's availability window against the project's supported
//! range and constructs the diagnostics, mirroring
//! `syntax_version_gating`'s shape.

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_semantics::{ResolutionOutcome, SemanticContext};

use crate::finding::{FindingAnchor, FindingSink};
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::SemanticRule;

/// A stub symbol introduced after the range minimum.
pub const SYMBOL_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0021");
/// A stub symbol removed at or before the range maximum.
pub const SYMBOL_REMOVED: DiagnosticId = DiagnosticId::new("CEL0022");
/// A stub symbol deprecated at the range maximum.
pub const SYMBOL_DEPRECATED: DiagnosticId = DiagnosticId::new("CEL0023");

/// The rule that reports stub symbols whose availability window does
/// not fully cover the project's supported PHP version range.
pub struct SymbolVersionGating;

/// The family's declarative unit. Severity is per identifier: the two
/// hard gates are errors, the deprecation is a warning (the mixed
/// severities design section 4 names).
pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "symbol-version-gating".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![
            RuleIdentifier {
                id: SYMBOL_NOT_AVAILABLE,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: SYMBOL_REMOVED,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: SYMBOL_DEPRECATED,
                severity: Severity::Warning,
            },
        ],
        tier: Tier::Default,
    }
}

impl SemanticRule for SymbolVersionGating {
    fn check(&self, context: &SemanticContext<'_>, sink: &mut FindingSink<'_>) {
        let version_range = context.php_version_range();
        for outcome in context.reference_resolutions() {
            let ResolutionOutcome::Stub { availability } = &outcome.resolution else {
                continue;
            };
            if let Some(introduced) = availability.introduced
                && introduced > version_range.minimum
            {
                sink.report(
                    SYMBOL_NOT_AVAILABLE,
                    FindingAnchor::Range(outcome.range),
                    format!(
                        "`{}` requires PHP {introduced}, but the project's minimum PHP version is {}",
                        outcome.written, version_range.minimum,
                    ),
                );
            }
            if let Some(removed) = availability.removed
                && removed <= version_range.maximum
            {
                sink.report(
                    SYMBOL_REMOVED,
                    FindingAnchor::Range(outcome.range),
                    format!(
                        "`{}` was removed in PHP {removed}, but the project's maximum PHP version is {}",
                        outcome.written, version_range.maximum,
                    ),
                );
            }
            if let Some(deprecation) = availability.deprecated {
                let applies = deprecation
                    .since
                    .is_none_or(|since| since <= version_range.maximum);
                if applies {
                    let message = match deprecation.since {
                        Some(since) => {
                            format!("`{}` is deprecated since PHP {since}", outcome.written)
                        }
                        None => format!("`{}` is deprecated", outcome.written),
                    };
                    sink.report(
                        SYMBOL_DEPRECATED,
                        FindingAnchor::Range(outcome.range),
                        message,
                    );
                }
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

    use celerrate_diagnostics::Severity;
    use celerrate_project::{PhpVersion, PhpVersionRange};
    use celerrate_stubs::{StubAvailability, StubSymbolKind};

    use super::{SYMBOL_DEPRECATED, SYMBOL_NOT_AVAILABLE, SYMBOL_REMOVED};
    use crate::rules::test_support::{semantic_diagnostics_in_range, stub_with};

    #[test]
    fn a_symbol_introduced_after_the_minimum_is_gated() {
        let diagnostics = semantic_diagnostics_in_range(
            &["<?php array_find([], fn($x) => $x);"],
            vec![stub_with(
                "array_find",
                StubSymbolKind::Function,
                StubAvailability {
                    introduced: Some(PhpVersion::new(8, 4)),
                    removed: None,
                    deprecated: None,
                },
            )],
            PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
        );
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostic.id, SYMBOL_NOT_AVAILABLE);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.message,
            "`array_find` requires PHP 8.4, but the project's minimum PHP version is 8.1",
        );
    }

    #[test]
    fn a_symbol_removed_within_the_range_is_gated() {
        let diagnostics = semantic_diagnostics_in_range(
            &["<?php utf8_encode('a');"],
            vec![stub_with(
                "utf8_encode",
                StubSymbolKind::Function,
                StubAvailability {
                    introduced: None,
                    removed: Some(PhpVersion::new(8, 3)),
                    deprecated: Some(celerrate_stubs::StubDeprecation {
                        since: Some(PhpVersion::new(8, 2)),
                    }),
                },
            )],
            PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
        );
        assert_eq!(diagnostics.len(), 2);
        let removed = diagnostics.iter().find(|d| d.id == SYMBOL_REMOVED).unwrap();
        assert_eq!(
            removed.message,
            "`utf8_encode` was removed in PHP 8.3, but the project's maximum PHP version is 8.5",
        );
        let deprecated = diagnostics
            .iter()
            .find(|d| d.id == SYMBOL_DEPRECATED)
            .unwrap();
        assert_eq!(deprecated.severity, Severity::Warning);
        assert_eq!(
            deprecated.message,
            "`utf8_encode` is deprecated since PHP 8.2"
        );
    }

    #[test]
    fn a_versionless_deprecation_still_warns() {
        let diagnostics = semantic_diagnostics_in_range(
            &["<?php old_helper();"],
            vec![stub_with(
                "old_helper",
                StubSymbolKind::Function,
                StubAvailability {
                    introduced: None,
                    removed: None,
                    deprecated: Some(celerrate_stubs::StubDeprecation { since: None }),
                },
            )],
            PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
        );
        assert_eq!(
            diagnostics.first().unwrap().message,
            "`old_helper` is deprecated"
        );
    }

    /// The silent half of the deprecation arm's `since` predicate: a
    /// symbol deprecated ABOVE the project's maximum is not deprecated
    /// for that project yet, so it must stay quiet. Nothing else
    /// reaches this branch. No other test supplies a `since` past the
    /// range maximum, and the shipped stub blob carries no deprecation
    /// after 8.5, so the corpus cannot exercise it either. Without this
    /// pin, inverting or dropping the comparison would ship a false
    /// positive with the whole suite green.
    #[test]
    fn a_deprecation_above_the_maximum_stays_silent() {
        let diagnostics = semantic_diagnostics_in_range(
            &["<?php future_helper();"],
            vec![stub_with(
                "future_helper",
                StubSymbolKind::Function,
                StubAvailability {
                    introduced: None,
                    removed: None,
                    deprecated: Some(celerrate_stubs::StubDeprecation {
                        since: Some(PhpVersion::new(8, 6)),
                    }),
                },
            )],
            PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
        );
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn a_project_declaration_is_never_gated() {
        let diagnostics = semantic_diagnostics_in_range(
            &["<?php function utf8_encode($s) { return $s; } utf8_encode('a');"],
            vec![stub_with(
                "utf8_encode",
                StubSymbolKind::Function,
                StubAvailability {
                    introduced: None,
                    removed: Some(PhpVersion::new(8, 3)),
                    deprecated: None,
                },
            )],
            PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
        );
        assert_eq!(diagnostics, vec![]);
    }

    /// The CEL0022 seeded-defect fixture (design section 1's recall
    /// gate). It lives here, not in `seeded_defects.rs`, because the
    /// shipped stub blob carries no removal inside the supported
    /// window 8.1 to 8.5, so no product-pipeline source can fire it;
    /// this drives the full framework path (core rules, the phase
    /// query) with a synthetic stub instead. Promote it to the
    /// product harness the day a real removal enters the window.
    #[test]
    fn cel0022_a_removed_symbol_is_reported_through_the_phase() {
        let diagnostics = semantic_diagnostics_in_range(
            &["<?php utf8_encode('a');"],
            vec![stub_with(
                "utf8_encode",
                StubSymbolKind::Function,
                StubAvailability {
                    introduced: None,
                    removed: Some(PhpVersion::new(8, 3)),
                    deprecated: None,
                },
            )],
            PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, SYMBOL_REMOVED);
        assert_eq!(
            diagnostics[0].message,
            "`utf8_encode` was removed in PHP 8.3, but the project's maximum PHP version is 8.5",
        );
    }
}
