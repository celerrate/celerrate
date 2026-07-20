//! Two diagnostic families over the statically named references of one
//! file: the unknown-symbol family (CEL0018-CEL0020), reporting a
//! reference that resolves to no declaration, and the symbol
//! version-gating family (CEL0021-CEL0023), reporting a reference that
//! resolves to a stub symbol whose availability window does not fully
//! cover the project's supported PHP version range. Two conservative
//! stances are documented engine semantics: dynamic references are out
//! of scope, and a symbol declared anywhere in project, vendor, or
//! stubs counts as declared, no reachability analysis of conditional
//! declarations.
//!
//! [`reference_outcomes`] walks the file's references exactly once,
//! resolving each name a single time and deriving both the diagnostics
//! above and the revalidation records of `revalidation.rs` from that
//! same resolution. `reference_diagnostics` and
//! `crate::revalidation::resolution_records` are thin projections over
//! it: one walk produces findings and answers, so drift between them is
//! structurally impossible — the `composed_diagnostics` closure,
//! applied to the second mirror, plan 9a.

use std::collections::HashMap;

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_project::{PhpVersionRange, ProjectConfiguration};
use celerrate_source::FileId;
use celerrate_stubs::{StubAvailability, StubIndexInput};

use crate::lookup::SymbolResolution;
use crate::queries::item_tree;
use crate::references::{Reference, collect_references};
use crate::resolve::{SymbolSources, UseTables, resolve_name};
use crate::revalidation::{ResolutionRecord, answer_of};
use crate::symbols::SymbolSpace;

/// A class-like reference that resolves to no declaration.
pub const UNKNOWN_CLASS: DiagnosticId = DiagnosticId::new("CEL0018");
/// A function call that resolves to no declaration.
pub const UNKNOWN_FUNCTION: DiagnosticId = DiagnosticId::new("CEL0019");
/// A constant reference that resolves to no declaration.
pub const UNKNOWN_CONSTANT: DiagnosticId = DiagnosticId::new("CEL0020");
/// A stub symbol introduced after the range minimum.
pub const SYMBOL_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0021");
/// A stub symbol removed at or before the range maximum.
pub const SYMBOL_REMOVED: DiagnosticId = DiagnosticId::new("CEL0022");
/// A stub symbol deprecated at the range maximum.
pub const SYMBOL_DEPRECATED: DiagnosticId = DiagnosticId::new("CEL0023");

/// Every identifier this crate allocates, for the registry check at the
/// composition root. `SYNTAX_NOT_AVAILABLE` lives in `syntax_gating`,
/// and joins the list here so there is exactly one list per crate.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[
    UNKNOWN_CLASS,
    UNKNOWN_FUNCTION,
    UNKNOWN_CONSTANT,
    SYMBOL_NOT_AVAILABLE,
    SYMBOL_REMOVED,
    SYMBOL_DEPRECATED,
    crate::syntax_gating::SYNTAX_NOT_AVAILABLE,
];

/// The findings and answers of one file's reference walk, produced by
/// [`reference_outcomes`] from the same pass over the same resolutions:
/// `diagnostics` is what `reference_diagnostics` used to compute alone,
/// `records` is what `resolution_records` used to compute alone. See
/// the module doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceOutcomes {
    pub diagnostics: Vec<Diagnostic>,
    pub records: Vec<ResolutionRecord>,
}

/// The single walk over the statically named references of one file:
/// for every reference, `resolve_name` runs exactly once, and its
/// result feeds both outputs — the diagnostic match that may report an
/// unknown-symbol or symbol version-gating finding, and the
/// revalidation record that reduces the same resolution to its answer.
/// Diagnostics are sorted; records keep walk (tree) order, the
/// convention `resolution_records`' tests pin.
#[salsa::tracked(returns(ref))]
pub fn reference_outcomes(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> ReferenceOutcomes {
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let tree = item_tree(db, file);
    let root = celerrate_db::parse(db, file).tree();
    let file_id = file.file_id(db);
    let version_range = configuration.php_version_range(db);
    let mut tables_by_namespace: HashMap<String, UseTables> = HashMap::new();
    let mut diagnostics = Vec::new();
    let mut records = Vec::new();
    for reference in collect_references(&root) {
        let tables = tables_by_namespace
            .entry(reference.namespace.clone())
            .or_insert_with(|| UseTables::for_namespace(tree, &reference.namespace));
        let resolution = resolve_name(
            db,
            sources,
            &reference.namespace,
            tables,
            &reference.written,
            reference.space,
        );
        records.push(ResolutionRecord {
            written: reference.written.clone(),
            space: reference.space,
            namespace: reference.namespace.clone(),
            answer: answer_of(resolution),
        });
        match resolution {
            None => diagnostics.push(unknown_symbol(&reference, file_id)),
            Some(SymbolResolution::Stub { availability, .. }) => {
                availability_diagnostics(
                    &reference,
                    availability,
                    version_range,
                    file_id,
                    &mut diagnostics,
                );
            }
            Some(SymbolResolution::Source { .. }) => {}
        }
    }
    diagnostics.sort();
    ReferenceOutcomes {
        diagnostics,
        records,
    }
}

/// The per-file reference diagnostics: for every statically named
/// reference, either an unknown-symbol diagnostic when it fails to
/// resolve, or a symbol version-gating diagnostic when it resolves to a
/// stub symbol whose availability does not fully cover the project's
/// supported PHP version range. A projection of [`reference_outcomes`]
/// (module doc): backdates independently of `resolution_records`, but
/// both read the same walk.
#[salsa::tracked(returns(ref))]
pub fn reference_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    reference_outcomes(db, file, files, stubs, configuration)
        .diagnostics
        .clone()
}

/// Emits diagnostics for a stub symbol whose availability window does
/// not fully cover the project's supported PHP version range: not yet
/// available at the minimum, removed at or before the maximum, or
/// deprecated at the maximum.
fn availability_diagnostics(
    reference: &Reference,
    availability: StubAvailability,
    version_range: PhpVersionRange,
    file: FileId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(introduced) = availability.introduced
        && introduced > version_range.minimum
    {
        diagnostics.push(Diagnostic::spanned(
            SYMBOL_NOT_AVAILABLE,
            Severity::Error,
            file,
            reference.range,
            format!(
                "`{}` requires PHP {introduced}, but the project's minimum PHP version is {}",
                reference.written, version_range.minimum,
            ),
        ));
    }
    if let Some(removed) = availability.removed
        && removed <= version_range.maximum
    {
        diagnostics.push(Diagnostic::spanned(
            SYMBOL_REMOVED,
            Severity::Error,
            file,
            reference.range,
            format!(
                "`{}` was removed in PHP {removed}, but the project's maximum PHP version is {}",
                reference.written, version_range.maximum,
            ),
        ));
    }
    if let Some(deprecation) = availability.deprecated {
        let applies = deprecation
            .since
            .is_none_or(|since| since <= version_range.maximum);
        if applies {
            let message = match deprecation.since {
                Some(since) => format!("`{}` is deprecated since PHP {since}", reference.written),
                None => format!("`{}` is deprecated", reference.written),
            };
            diagnostics.push(Diagnostic::spanned(
                SYMBOL_DEPRECATED,
                Severity::Warning,
                file,
                reference.range,
                message,
            ));
        }
    }
}

fn unknown_symbol(reference: &Reference, file: FileId) -> Diagnostic {
    let (id, kind) = match reference.space {
        SymbolSpace::ClassLike => (UNKNOWN_CLASS, "class"),
        SymbolSpace::Function => (UNKNOWN_FUNCTION, "function"),
        SymbolSpace::Constant => (UNKNOWN_CONSTANT, "constant"),
    };
    Diagnostic::spanned(
        id,
        Severity::Error,
        file,
        reference.range,
        format!("unknown {kind} `{}`", reference.written),
    )
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    fn stub(name: &str, kind: StubSymbolKind) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind,
            availability: StubAvailability::ALWAYS,
        }
    }

    fn stub_with(name: &str, kind: StubSymbolKind, availability: StubAvailability) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind,
            availability,
        }
    }

    /// The diagnostics of the FIRST source, with the given stubs and
    /// the given supported range.
    fn checked_in_range(
        sources: &[&str],
        stub_symbols: Vec<StubSymbol>,
        range: PhpVersionRange,
    ) -> Vec<Diagnostic> {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let file = *handles.first().unwrap();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(stub_symbols))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(range)
            .durability(salsa::Durability::MEDIUM)
            .new(&db);
        reference_diagnostics(&db, file, files, stubs, configuration).clone()
    }

    /// The full supported range used by tests that do not exercise
    /// version gating.
    fn full_range() -> PhpVersionRange {
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5))
    }

    #[test]
    fn an_unresolved_class_is_reported_at_its_written_name() {
        let source = "<?php namespace App; $x = new Client();";
        let diagnostics = checked_in_range(&[source], vec![], full_range());
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
            checked_in_range(
                &[
                    "<?php namespace App; use Lib\\Helper; $x = new Helper();",
                    "<?php namespace Lib; class Helper {}",
                ],
                vec![],
                full_range(),
            ),
            vec![],
        );
    }

    #[test]
    fn a_stub_declaration_counts() {
        assert_eq!(
            checked_in_range(
                &["<?php $x = strlen('a'); $t = new \\ArrayObject();"],
                vec![
                    stub("strlen", StubSymbolKind::Function),
                    stub("ArrayObject", StubSymbolKind::Class),
                ],
                full_range(),
            ),
            vec![],
        );
    }

    #[test]
    fn an_unresolved_alias_reports_the_written_name() {
        let diagnostics = checked_in_range(
            &["<?php use Lib\\Missing as M; $x = new M();"],
            vec![],
            full_range(),
        );
        assert_eq!(diagnostics.first().unwrap().message, "unknown class `M`");
    }

    #[test]
    fn functions_fall_back_to_the_global_namespace() {
        let diagnostics = checked_in_range(
            &["<?php namespace App; strlen('a'); missing('b');"],
            vec![stub("strlen", StubSymbolKind::Function)],
            full_range(),
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
        let diagnostics = checked_in_range(
            &["<?php $a = PHP_EOL; $b = php_eol;"],
            vec![stub("PHP_EOL", StubSymbolKind::Constant)],
            full_range(),
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
            checked_in_range(
                &["<?php if (!function_exists('helper')) { function helper() {} } helper();"],
                vec![stub("function_exists", StubSymbolKind::Function)],
                full_range(),
            ),
            vec![],
        );
    }

    #[test]
    fn a_symbol_introduced_after_the_minimum_is_gated() {
        let diagnostics = checked_in_range(
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
        let diagnostics = checked_in_range(
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
        let diagnostics = checked_in_range(
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

    #[test]
    fn a_symbol_absent_from_the_whole_range_is_unknown_not_gated() {
        // Removed at or before the minimum: filtered out of the stub table
        // by stubs_in_range, so the reference reports unknown symbol.
        let diagnostics = checked_in_range(
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
    fn a_project_declaration_is_never_gated() {
        let diagnostics = checked_in_range(
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

    #[test]
    fn a_dynamically_named_define_is_not_indexed() {
        let diagnostics = checked_in_range(
            &["<?php define($name, 1); echo APP_ROOT;"],
            vec![stub("define", StubSymbolKind::Function)],
            full_range(),
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, UNKNOWN_CONSTANT);
    }

    #[test]
    fn a_define_keeps_its_terminal_segment_case_sensitive() {
        let diagnostics = checked_in_range(
            &["<?php define('APP_ROOT', 1); echo App_Root;"],
            vec![stub("define", StubSymbolKind::Function)],
            full_range(),
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].id, UNKNOWN_CONSTANT);
    }
}
