//! The per-name lookup: the invalidation firewall of the symbol index.
//! Consumers never depend on a whole table; they ask for one interned
//! name. When signatures change somewhere, the table query re-runs and
//! every cached lookup re-runs too, but a lookup is a binary search:
//! cheap, and backdated whenever its answer did not change, so the
//! consumers behind it are spared. Adding a symbol in one file never
//! re-analyzes files that do not reference it.

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::{StubAvailability, StubIndexInput, StubSymbolKind};

use crate::index::{source_symbol_table, stub_symbol_table};
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
}
