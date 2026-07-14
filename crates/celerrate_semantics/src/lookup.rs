//! The per-name lookup: the invalidation firewall of the symbol index.
//! Consumers never depend on a whole table; they ask for one interned
//! name. When signatures change somewhere, the table query re-runs and
//! every cached lookup re-runs too, but a lookup is a binary search:
//! cheap, and backdated whenever its answer did not change, so the
//! consumers behind it are spared. Adding a symbol in one file never
//! re-analyzes files that do not reference it.

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_source::FileId;
use celerrate_stubs::{StubAvailability, StubIndexInput, StubSymbolKind};

use crate::ast_id::AstId;
use crate::index::{SymbolOrigin, source_symbol_table, stub_symbol_table};
use crate::items::DeclarationKind;
use crate::symbols::SymbolSpace;

/// One name to look up: its space and its **pre-folded** key (fold
/// with [`crate::folded_symbol_key`] before interning, so spelling
/// variants of one name share one memo).
#[salsa::interned(debug)]
pub struct SymbolQuery<'db> {
    pub space: SymbolSpace,
    #[returns(ref)]
    pub key: String,
}

/// Where a name resolved: a declaration of the analyzed file set, or a
/// compiled stub with its availability window (part 6's version gating
/// reads it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolResolution {
    Source {
        kind: DeclarationKind,
    },
    Stub {
        kind: StubSymbolKind,
        availability: StubAvailability,
    },
}

/// Resolves one folded key against the global index: the source table
/// first (a project declaration shadows a stub), the stubs second.
#[salsa::tracked]
pub fn lookup_symbol<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: SymbolQuery<'db>,
) -> Option<SymbolResolution> {
    let space = query.space(db);
    let key = query.key(db);
    if let Some(entry) = source_symbol_table(db, files).lookup(space, key) {
        return Some(SymbolResolution::Source { kind: entry.kind });
    }
    stub_symbol_table(db, stubs, configuration)
        .lookup(space, key)
        .map(|entry| SymbolResolution::Stub {
            kind: entry.symbol.kind,
            availability: entry.symbol.availability,
        })
}

/// The analyzed files by identifier, sorted: the bridge from an
/// `AstId` (which carries a `FileId`) back to the salsa handle whose
/// trees can be asked for. Depends on the file *set*, not on any
/// file's content, so content edits never re-run it.
#[salsa::tracked(returns(ref))]
pub fn analyzed_file_index(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
) -> Vec<(FileId, SourceFile)> {
    let mut index: Vec<(FileId, SourceFile)> = files
        .files(db)
        .iter()
        .map(|&file| (file.file_id(db), file))
        .collect();
    index.sort_by_key(|(id, _)| *id);
    index
}

/// The declaring identity of one source class-like: the same firewall
/// as `lookup_symbol`, answering the origin instead of the kind alone.
/// `None` for stub symbols (no source declaration), for `define()`
/// origins (not class-likes), and for unknown names.
#[salsa::tracked]
pub fn lookup_class_declaration<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    query: SymbolQuery<'db>,
) -> Option<(DeclarationKind, AstId)> {
    let entry = source_symbol_table(db, files).lookup(query.space(db), query.key(db))?;
    match entry.origin {
        SymbolOrigin::Item(ast_id) => Some((entry.kind, ast_id)),
        SymbolOrigin::Define(_) => None,
    }
}

/// The declaring identity of one source function: the same firewall as
/// `lookup_class_declaration`, restricted to `DeclarationKind::Function`.
/// `None` for stubs, non-functions, `define()` origins, and unknown
/// names.
#[salsa::tracked]
pub fn lookup_function_declaration<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    query: SymbolQuery<'db>,
) -> Option<AstId> {
    let entry = source_symbol_table(db, files).lookup(query.space(db), query.key(db))?;
    if entry.kind != DeclarationKind::Function {
        return None;
    }
    match entry.origin {
        SymbolOrigin::Item(ast_id) => Some(ast_id),
        SymbolOrigin::Define(_) => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use super::{SymbolQuery, SymbolResolution, lookup_symbol};
    use crate::items::DeclarationKind;
    use crate::symbols::{SymbolSpace, folded_symbol_key};

    struct Fixture {
        db: TestDatabase,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    }

    fn fixture(sources: &[&str]) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![
            StubSymbol {
                name: "Exception".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
        ]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Fixture {
            db,
            files,
            stubs,
            configuration,
        }
    }

    fn resolve(fixture: &Fixture, space: SymbolSpace, written: &str) -> Option<SymbolResolution> {
        let query = SymbolQuery::new(&fixture.db, space, folded_symbol_key(space, written));
        lookup_symbol(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
    }

    #[test]
    fn a_project_declaration_resolves() {
        let fixture = fixture(&["<?php namespace App; class Service {}"]);
        assert_eq!(
            resolve(&fixture, SymbolSpace::ClassLike, "APP\\SERVICE"),
            Some(SymbolResolution::Source {
                kind: DeclarationKind::Class,
            }),
        );
    }

    #[test]
    fn a_stub_symbol_resolves_with_its_availability() {
        let fixture = fixture(&["<?php"]);
        let resolution = resolve(&fixture, SymbolSpace::Function, "strlen");
        assert_eq!(
            resolution,
            Some(SymbolResolution::Stub {
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            }),
        );
    }

    #[test]
    fn a_project_declaration_shadows_the_stub() {
        // A polyfill declaring a standard symbol wins over the stubs.
        let fixture = fixture(&["<?php class Exception {}"]);
        assert_eq!(
            resolve(&fixture, SymbolSpace::ClassLike, "Exception"),
            Some(SymbolResolution::Source {
                kind: DeclarationKind::Class,
            }),
        );
    }

    #[test]
    fn an_unknown_name_answers_none() {
        let fixture = fixture(&["<?php class Known {}"]);
        assert_eq!(resolve(&fixture, SymbolSpace::ClassLike, "Unknown"), None);
        assert_eq!(resolve(&fixture, SymbolSpace::Function, "unknown"), None);
    }

    use crate::lookup::{analyzed_file_index, lookup_class_declaration};

    fn class_declaration(
        fixture: &Fixture,
        written: &str,
    ) -> Option<(DeclarationKind, crate::AstId)> {
        let space = SymbolSpace::ClassLike;
        let query = SymbolQuery::new(&fixture.db, space, folded_symbol_key(space, written));
        lookup_class_declaration(&fixture.db, fixture.files, query)
    }

    #[test]
    fn a_source_class_answers_its_declaring_identity() {
        let fixture = fixture(&["<?php namespace App; class Service {}"]);
        let (kind, ast_id) = class_declaration(&fixture, "App\\Service").unwrap();
        assert_eq!(kind, DeclarationKind::Class);
        assert_eq!(ast_id.file, FileId::new(0));
    }

    #[test]
    fn a_stub_only_class_answers_none_here() {
        // `lookup_class_declaration` is source-only: a stub class-like
        // answers `None` here. The stub graph is consulted through
        // `stub_signature_table` (linearization and member lookup), not
        // through this source-declaration firewall.
        let fixture = fixture(&["<?php"]);
        assert_eq!(class_declaration(&fixture, "Exception"), None);
    }

    #[test]
    fn the_file_index_maps_ids_to_handles_sorted() {
        let fixture = fixture(&["<?php class A {}", "<?php class B {}"]);
        let index = analyzed_file_index(&fixture.db, fixture.files);
        let ids: Vec<u32> = index.iter().map(|(id, _)| id.as_u32()).collect();
        assert_eq!(ids, vec![0, 1]);
    }

    use crate::lookup::lookup_function_declaration;

    #[test]
    fn a_source_function_answers_its_declaring_identity() {
        let fixture = fixture(&["<?php namespace App; function build(): int {}"]);
        let space = SymbolSpace::Function;
        let query = SymbolQuery::new(&fixture.db, space, folded_symbol_key(space, "App\\build"));
        let ast_id = lookup_function_declaration(&fixture.db, fixture.files, query);
        assert!(ast_id.is_some());
        // A stub function has no source declaration.
        let stub_query = SymbolQuery::new(&fixture.db, space, folded_symbol_key(space, "strlen"));
        assert!(lookup_function_declaration(&fixture.db, fixture.files, stub_query).is_none());
    }
}
