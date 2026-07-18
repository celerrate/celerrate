//! The revalidation records of one file: which names its reference
//! checks looked up, and what each lookup answered, reduced to exactly
//! what the diagnostics depend on. A persisted diagnostics entry is
//! accepted only when every recorded answer still holds; the records
//! are what makes "deserialize plus revalidate" a sound substitute for
//! recomputation.
//!
//! `resolution_records` is a thin projection over
//! `crate::reference_checks::reference_outcomes`: one walk produces
//! findings and answers together, so drift between this module's
//! records and `reference_checks`' diagnostics is structurally
//! impossible — the `composed_diagnostics` closure, applied to the
//! second mirror, plan 9a.

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::{StubAvailability, StubIndexInput};

use crate::lookup::SymbolResolution;
use crate::reference_checks::reference_outcomes;
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
/// answer, in tree order. A projection of
/// `crate::reference_checks::reference_outcomes` (module doc):
/// backdates independently of `reference_diagnostics`, but both read
/// the same walk.
#[salsa::tracked(returns(ref))]
pub fn resolution_records(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<ResolutionRecord> {
    reference_outcomes(db, file, files, stubs, configuration)
        .records
        .clone()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use crate::reference_checks::{
        SYMBOL_DEPRECATED, SYMBOL_NOT_AVAILABLE, SYMBOL_REMOVED, UNKNOWN_CLASS, UNKNOWN_CONSTANT,
        UNKNOWN_FUNCTION, reference_outcomes,
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

    /// The name a diagnostic message backtick-quotes first: every
    /// message this crate emits carries the reference's written name
    /// as its first backtick-quoted token, whether the name opens the
    /// message (gating) or sits mid-sentence (unknown-symbol).
    fn written_name(message: &str) -> &str {
        message.split('`').nth(1).unwrap_or_default()
    }

    /// The drift pin: `reference_outcomes` runs one walk that produces
    /// findings and answers together, so every diagnostic it reports
    /// must be explained by a record from the very same call — a
    /// correspondence the two hand-maintained walks this replaces could
    /// never guarantee. The fixture carries one unknown class, one
    /// source-resolved class, and one stub reference whose availability
    /// window violates the project's supported range.
    #[test]
    fn findings_and_answers_come_from_one_walk() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php class Known {} $a = new Known(); $b = new Missing(); \
              array_find([], fn($x) => $x);"
                .to_vec(),
        );
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![StubSymbol {
            name: "array_find".to_owned(),
            kind: StubSymbolKind::Function,
            availability: StubAvailability {
                introduced: Some(PhpVersion::new(8, 4)),
                removed: None,
                deprecated: None,
            },
        }]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);

        let outcomes = reference_outcomes(&db, file, files, stubs, configuration);
        let records = resolution_records(&db, file, files, stubs, configuration);

        assert_eq!(
            outcomes.records.len(),
            records.len(),
            "resolution_records is a total projection of reference_outcomes: {:?} vs {:?}",
            outcomes.records,
            records,
        );
        assert_eq!(
            outcomes.diagnostics.len(),
            2,
            "one unknown class and one gated stub reference: {:?}",
            outcomes.diagnostics,
        );

        for diagnostic in &outcomes.diagnostics {
            let written = written_name(&diagnostic.message);
            let record = outcomes
                .records
                .iter()
                .find(|record| record.written == written)
                .unwrap_or_else(|| panic!("no record explains diagnostic {diagnostic:?}"));
            match diagnostic.id {
                id if id == UNKNOWN_CLASS || id == UNKNOWN_FUNCTION || id == UNKNOWN_CONSTANT => {
                    assert_eq!(
                        record.answer,
                        ResolutionAnswer::Unknown,
                        "an unknown-symbol diagnostic must be explained by an unknown answer",
                    );
                }
                id if id == SYMBOL_NOT_AVAILABLE
                    || id == SYMBOL_REMOVED
                    || id == SYMBOL_DEPRECATED =>
                {
                    assert!(
                        matches!(record.answer, ResolutionAnswer::Stub { .. }),
                        "a gating diagnostic must be explained by a stub answer",
                    );
                }
                other => panic!("unexpected diagnostic id {other:?}"),
            }
        }
    }
}
