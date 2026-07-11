//! The unknown-symbol family: every statically named reference of one
//! file, resolved; an unresolved reference is a diagnostic. Two
//! conservative stances are documented engine semantics: dynamic
//! references are out of scope, and a symbol declared anywhere in
//! project, vendor, or stubs counts as declared — no reachability
//! analysis of conditional declarations.

use std::collections::HashMap;

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::lookup::SymbolResolution;
use crate::queries::item_tree;
use crate::references::{Reference, collect_references};
use crate::resolve::{SymbolSources, UseTables, resolve_name};
use crate::symbols::SymbolSpace;

/// A class-like reference that resolves to no declaration.
pub const UNKNOWN_CLASS: DiagnosticId = DiagnosticId::new("CEL0018");
/// A function call that resolves to no declaration.
pub const UNKNOWN_FUNCTION: DiagnosticId = DiagnosticId::new("CEL0019");
/// A constant reference that resolves to no declaration.
pub const UNKNOWN_CONSTANT: DiagnosticId = DiagnosticId::new("CEL0020");

/// The per-file reference diagnostics: unknown symbols now, the symbol
/// version-gating family joins in the same pass (task 6).
#[salsa::tracked(returns(ref))]
pub fn reference_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let tree = item_tree(db, file);
    let root = celerrate_db::parse(db, file).tree();
    let file_id = file.file_id(db);
    let mut tables_by_namespace: HashMap<String, UseTables> = HashMap::new();
    let mut diagnostics = Vec::new();
    for reference in collect_references(&root) {
        let tables = tables_by_namespace
            .entry(reference.namespace.clone())
            .or_insert_with(|| UseTables::for_namespace(tree, &reference.namespace));
        match resolve_name(
            db,
            sources,
            &reference.namespace,
            tables,
            &reference.written,
            reference.space,
        ) {
            None => diagnostics.push(unknown_symbol(&reference, file_id)),
            Some(SymbolResolution::Stub { .. }) => {}
            Some(SymbolResolution::Source { .. }) => {}
        }
    }
    diagnostics.sort();
    diagnostics
}

fn unknown_symbol(reference: &Reference, file: celerrate_source::FileId) -> Diagnostic {
    let (id, kind) = match reference.space {
        SymbolSpace::ClassLike => (UNKNOWN_CLASS, "class"),
        SymbolSpace::Function => (UNKNOWN_FUNCTION, "function"),
        SymbolSpace::Constant => (UNKNOWN_CONSTANT, "constant"),
    };
    Diagnostic {
        id,
        severity: Severity::Error,
        file,
        range: reference.range,
        message: format!("unknown {kind} `{}`", reference.written),
    }
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

    /// The diagnostics of the FIRST source, with the given stubs and
    /// the full supported range.
    fn checked(sources: &[&str], stub_symbols: Vec<StubSymbol>) -> Vec<Diagnostic> {
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
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        reference_diagnostics(&db, file, files, stubs, configuration).clone()
    }

    #[test]
    fn an_unresolved_class_is_reported_at_its_written_name() {
        let source = "<?php namespace App; $x = new Client();";
        let diagnostics = checked(&[source], vec![]);
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostic.id, UNKNOWN_CLASS);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.message, "unknown class `Client`");
        let start: usize = diagnostic.range.start().into();
        let end: usize = diagnostic.range.end().into();
        assert_eq!(&source[start..end], "Client");
    }

    #[test]
    fn a_declaration_anywhere_in_the_file_set_counts() {
        assert_eq!(
            checked(
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
            checked(
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
        let diagnostics = checked(&["<?php use Lib\\Missing as M; $x = new M();"], vec![]);
        assert_eq!(diagnostics.first().unwrap().message, "unknown class `M`");
    }

    #[test]
    fn functions_fall_back_to_the_global_namespace() {
        let diagnostics = checked(
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
        let diagnostics = checked(
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
            checked(
                &["<?php if (!function_exists('helper')) { function helper() {} } helper();"],
                vec![stub("function_exists", StubSymbolKind::Function)],
            ),
            vec![],
        );
    }
}
