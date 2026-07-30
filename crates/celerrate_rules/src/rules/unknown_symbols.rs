//! The unknown-symbol family, migrated: the
//! resolution walk lives below as
//! `celerrate_semantics::reference_outcomes`; this rule consumes the
//! per-reference outcomes and constructs the diagnostics.

use celerrate_diagnostics::{DiagnosticId, Severity};
use celerrate_semantics::{ResolutionOutcome, SemanticContext, SymbolSpace};

use crate::finding::{FindingAnchor, FindingSink};
use crate::metadata::{RuleGroup, RuleIdentifier, RuleMetadata, Tier};
use crate::traits::SemanticRule;

/// A class-like reference that resolves to no declaration.
pub const UNKNOWN_CLASS: DiagnosticId = DiagnosticId::new("CEL0018");
/// A function call that resolves to no declaration.
pub const UNKNOWN_FUNCTION: DiagnosticId = DiagnosticId::new("CEL0019");
/// A constant reference that resolves to no declaration.
pub const UNKNOWN_CONSTANT: DiagnosticId = DiagnosticId::new("CEL0020");

/// The rule that reports statically named references resolving to no
/// declaration anywhere in project, vendor, or stubs. The two
/// conservative stances are the walk's, documented there: dynamic
/// references are out of scope, and conditional declarations count.
pub struct UnknownSymbols;

/// The family's declarative unit.
pub fn metadata() -> RuleMetadata {
    RuleMetadata {
        name: "unknown-symbols".to_owned(),
        group: RuleGroup::Correctness,
        identifiers: vec![
            RuleIdentifier {
                id: UNKNOWN_CLASS,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: UNKNOWN_FUNCTION,
                severity: Severity::Error,
            },
            RuleIdentifier {
                id: UNKNOWN_CONSTANT,
                severity: Severity::Error,
            },
        ],
        tier: Tier::Default,
    }
}

impl SemanticRule for UnknownSymbols {
    fn check(&self, context: &SemanticContext<'_>, sink: &mut FindingSink<'_>) {
        for outcome in context.reference_resolutions() {
            if !matches!(outcome.resolution, ResolutionOutcome::Unresolved) {
                continue;
            }
            let (id, kind) = match outcome.space {
                SymbolSpace::ClassLike => (UNKNOWN_CLASS, "class"),
                SymbolSpace::Function => (UNKNOWN_FUNCTION, "function"),
                SymbolSpace::Constant => (UNKNOWN_CONSTANT, "constant"),
            };
            sink.report(
                id,
                FindingAnchor::Range(outcome.range),
                format!("unknown {kind} `{}`", outcome.written),
            );
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

    use super::{UNKNOWN_CLASS, UNKNOWN_CONSTANT, UNKNOWN_FUNCTION};
    use crate::rules::test_support::{
        semantic_diagnostics, semantic_diagnostics_in_range, stub, stub_with,
    };

    #[test]
    fn an_unresolved_class_is_reported_at_its_written_name() {
        let source = "<?php namespace App; $x = new Client();";
        let diagnostics = semantic_diagnostics(&[source], vec![]);
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostic.id, UNKNOWN_CLASS);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.message, "unknown class `Client`");
        let (_, range) = diagnostic.span().unwrap();
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&source[start..end], "Client");
    }

    #[test]
    fn a_declaration_anywhere_in_the_file_set_counts() {
        assert_eq!(
            semantic_diagnostics(
                &[
                    "<?php namespace App; use Lib\\Helper; $x = new Helper();",
                    "<?php namespace Lib; class Helper {}",
                ],
                vec![],
            ),
            vec![],
        );
    }

    #[test]
    fn a_stub_declaration_counts() {
        assert_eq!(
            semantic_diagnostics(
                &["<?php $x = strlen('a'); $t = new \\ArrayObject();"],
                vec![
                    stub("strlen", StubSymbolKind::Function),
                    stub("ArrayObject", StubSymbolKind::Class),
                ],
            ),
            vec![],
        );
    }

    #[test]
    fn an_unresolved_alias_reports_the_written_name() {
        let diagnostics =
            semantic_diagnostics(&["<?php use Lib\\Missing as M; $x = new M();"], vec![]);
        assert_eq!(diagnostics.first().unwrap().message, "unknown class `M`");
    }

    #[test]
    fn functions_fall_back_to_the_global_namespace() {
        let diagnostics = semantic_diagnostics(
            &["<?php namespace App; strlen('a'); missing('b');"],
            vec![stub("strlen", StubSymbolKind::Function)],
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics.first().unwrap().id, UNKNOWN_FUNCTION);
        assert_eq!(
            diagnostics.first().unwrap().message,
            "unknown function `missing`"
        );
    }

    #[test]
    fn constant_terminal_segments_stay_case_sensitive() {
        let diagnostics = semantic_diagnostics(
            &["<?php $a = PHP_EOL; $b = php_eol;"],
            vec![stub("PHP_EOL", StubSymbolKind::Constant)],
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics.first().unwrap().id, UNKNOWN_CONSTANT);
        assert_eq!(
            diagnostics.first().unwrap().message,
            "unknown constant `php_eol`"
        );
    }

    #[test]
    fn a_conditionally_declared_symbol_counts_as_declared() {
        assert_eq!(
            semantic_diagnostics(
                &["<?php if (!function_exists('helper')) { function helper() {} } helper();"],
                vec![stub("function_exists", StubSymbolKind::Function)],
            ),
            vec![],
        );
    }

    #[test]
    fn a_symbol_absent_from_the_whole_range_is_unknown_not_gated() {
        // Removed at or before the minimum: filtered out of the stub table
        // by stubs_in_range, so the reference reports unknown symbol.
        let diagnostics = semantic_diagnostics_in_range(
            &["<?php ancient();"],
            vec![stub_with(
                "ancient",
                StubSymbolKind::Function,
                StubAvailability {
                    introduced: None,
                    removed: Some(PhpVersion::new(8, 1)),
                    deprecated: None,
                },
            )],
            PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5)),
        );
        assert_eq!(diagnostics.first().unwrap().id, UNKNOWN_FUNCTION);
    }

    #[test]
    fn a_dynamically_named_define_is_not_indexed() {
        let diagnostics = semantic_diagnostics(
            &["<?php define($name, 1); echo APP_ROOT;"],
            vec![stub("define", StubSymbolKind::Function)],
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, UNKNOWN_CONSTANT);
    }

    #[test]
    fn a_define_keeps_its_terminal_segment_case_sensitive() {
        let diagnostics = semantic_diagnostics(
            &["<?php define('APP_ROOT', 1); echo App_Root;"],
            vec![stub("define", StubSymbolKind::Function)],
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, UNKNOWN_CONSTANT);
    }
}
