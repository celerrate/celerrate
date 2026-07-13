//! The revalidation records of one file: which names its reference
//! checks looked up, and what each lookup answered, reduced to exactly
//! what the diagnostics depend on. A persisted diagnostics entry is
//! accepted only when every recorded answer still holds; the records
//! are what makes "deserialize plus revalidate" a sound substitute for
//! recomputation.

use std::collections::HashMap;

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::{StubAvailability, StubIndexInput};

use crate::lookup::SymbolResolution;
use crate::queries::item_tree;
use crate::references::collect_references;
use crate::resolve::{SymbolSources, UseTables, resolve_name};
use crate::symbols::SymbolSpace;

/// The answer a resolution reduces to: exactly what the reference
/// diagnostics depend on, and nothing more. A `Source` answer produces
/// no diagnostic whatever its declaration kind, so the kind is
/// dropped; a `Stub` answer's diagnostics are a function of its
/// availability window, so the window is kept whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionAnswer {
    Unknown,
    Source,
    Stub { availability: StubAvailability },
}

/// Reduces a resolution to its answer.
pub fn answer_of(resolution: Option<SymbolResolution>) -> ResolutionAnswer {
    match resolution {
        None => ResolutionAnswer::Unknown,
        Some(SymbolResolution::Source { .. }) => ResolutionAnswer::Source,
        Some(SymbolResolution::Stub { availability, .. }) => {
            ResolutionAnswer::Stub { availability }
        }
    }
}

/// One reference with its answer: what a persisted diagnostics entry
/// must re-check before it may speak for this file again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionRecord {
    pub written: String,
    pub space: SymbolSpace,
    pub namespace: String,
    pub answer: ResolutionAnswer,
}

/// Every statically named reference of the file with its current
/// answer, in tree order. The same traversal and resolution path as
/// `reference_diagnostics`, reduced to answers instead of findings.
#[salsa::tracked(returns(ref))]
pub fn resolution_records(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<ResolutionRecord> {
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let tree = item_tree(db, file);
    let root = celerrate_db::parse(db, file).tree();
    let mut tables_by_namespace: HashMap<String, UseTables> = HashMap::new();
    let mut records = Vec::new();
    for reference in collect_references(&root) {
        let tables = tables_by_namespace
            .entry(reference.namespace.clone())
            .or_insert_with(|| UseTables::for_namespace(tree, &reference.namespace));
        let answer = answer_of(resolve_name(
            db,
            sources,
            &reference.namespace,
            tables,
            &reference.written,
            reference.space,
        ));
        records.push(ResolutionRecord {
            written: reference.written,
            space: reference.space,
            namespace: reference.namespace,
            answer,
        });
    }
    records
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use crate::revalidation::{ResolutionAnswer, resolution_records};
    use crate::symbols::SymbolSpace;

    fn configuration(db: &TestDatabase) -> ProjectConfiguration {
        ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(db)
    }

    fn stub_index_with_strlen(db: &TestDatabase) -> StubIndexInput {
        let index = StubIndex::from_symbols(vec![StubSymbol {
            name: "strlen".to_owned(),
            kind: StubSymbolKind::Function,
            availability: StubAvailability::ALWAYS,
        }]);
        StubIndexInput::builder(index)
            .durability(salsa::Durability::HIGH)
            .new(db)
    }

    #[test]
    fn every_reference_is_recorded_with_its_answer() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php class Known {} $a = new Known(); $b = new Missing(); $c = strlen('x');"
                .to_vec(),
        );
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let stubs = stub_index_with_strlen(&db);
        let configuration = configuration(&db);

        let records = resolution_records(&db, file, files, stubs, configuration);
        let summary: Vec<(&str, SymbolSpace, ResolutionAnswer)> = records
            .iter()
            .map(|record| (record.written.as_str(), record.space, record.answer))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Known", SymbolSpace::ClassLike, ResolutionAnswer::Source),
                ("Missing", SymbolSpace::ClassLike, ResolutionAnswer::Unknown),
                (
                    "strlen",
                    SymbolSpace::Function,
                    ResolutionAnswer::Stub {
                        availability: StubAvailability::ALWAYS,
                    }
                ),
            ],
        );
    }

    #[test]
    fn an_answer_flips_when_a_defining_file_appears() {
        use salsa::Setter as _;

        let mut db = TestDatabase::default();
        let referencing = SourceFile::new(&db, FileId::new(0), b"<?php new Missing();".to_vec());
        let other = SourceFile::new(&db, FileId::new(1), b"<?php".to_vec());
        let files = AnalyzedFileSet::new(&db, vec![referencing, other]);
        let stubs = stub_index_with_strlen(&db);
        let configuration = configuration(&db);

        let before = resolution_records(&db, referencing, files, stubs, configuration);
        assert_eq!(before[0].answer, ResolutionAnswer::Unknown);

        other
            .set_bytes(&mut db)
            .to(b"<?php class Missing {}".to_vec());
        let after = resolution_records(&db, referencing, files, stubs, configuration);
        assert_eq!(after[0].answer, ResolutionAnswer::Source);
    }
}
