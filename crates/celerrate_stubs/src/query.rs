//! The salsa surface of the stubs: the index as a high-durability
//! input and the version-filtered view as a tracked query.

use celerrate_project::ProjectConfiguration;

use crate::index::StubIndex;

/// The decoded stub index as a salsa input. Created once at the
/// composition root with `salsa::Durability::HIGH`: it changes only
/// when the binary (or, later, an overlay) changes.
#[salsa::input]
pub struct StubIndexInput {
    #[returns(ref)]
    pub index: StubIndex,
}

/// The stub symbols that exist somewhere in the configured version
/// range. Symbols removed before the minimum or introduced after the
/// maximum are invisible; availability metadata stays on the
/// survivors — part 6's version-gating checks read it.
#[salsa::tracked(returns(ref))]
pub fn stubs_in_range(
    db: &dyn salsa::Database,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> StubIndex {
    let range = configuration.php_version_range(db);
    StubIndex::from_symbols(
        stubs
            .index(db)
            .symbols()
            .iter()
            .filter(|symbol| symbol.availability.exists_in(range))
            .cloned()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use salsa::Setter;

    use super::{StubIndexInput, stubs_in_range};
    use crate::index::StubIndex;
    use crate::symbol::{StubAvailability, StubSymbol, StubSymbolKind};

    fn sample_input(db: &TestDatabase) -> StubIndexInput {
        let index = StubIndex::from_symbols(vec![
            StubSymbol {
                name: "always_there".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "born_in_php_84".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability {
                    introduced: Some(PhpVersion::new(8, 4)),
                    ..StubAvailability::ALWAYS
                },
            },
            StubSymbol {
                name: "gone_in_php_80".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability {
                    removed: Some(PhpVersion::new(8, 0)),
                    ..StubAvailability::ALWAYS
                },
            },
        ]);
        StubIndexInput::builder(index)
            .durability(salsa::Durability::HIGH)
            .new(db)
    }

    fn configuration(
        db: &TestDatabase,
        minimum: (u8, u8),
        maximum: (u8, u8),
    ) -> ProjectConfiguration {
        ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(minimum.0, minimum.1),
            PhpVersion::new(maximum.0, maximum.1),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(db)
    }

    fn filtered_names(
        db: &TestDatabase,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    ) -> Vec<String> {
        stubs_in_range(db, stubs, configuration)
            .symbols()
            .iter()
            .map(|symbol| symbol.name.clone())
            .collect()
    }

    #[test]
    fn the_filtered_view_keeps_only_symbols_that_exist_in_the_range() {
        let db = TestDatabase::default();
        let stubs = sample_input(&db);
        let configuration = configuration(&db, (8, 1), (8, 3));
        assert_eq!(
            filtered_names(&db, stubs, configuration),
            vec!["always_there".to_owned()],
        );
    }

    #[test]
    fn widening_the_range_reveals_more_symbols() {
        let db = TestDatabase::default();
        let stubs = sample_input(&db);
        let configuration = configuration(&db, (8, 1), (8, 5));
        assert_eq!(
            filtered_names(&db, stubs, configuration),
            vec!["always_there".to_owned(), "born_in_php_84".to_owned()],
        );
    }

    #[test]
    fn changing_the_version_range_recomputes_the_filtered_view() {
        let mut db = TestDatabase::default();
        let stubs = sample_input(&db);
        let configuration = configuration(&db, (8, 1), (8, 3));
        let _ = stubs_in_range(&db, stubs, configuration);
        db.take_executed();

        configuration
            .set_php_version_range(&mut db)
            .to(PhpVersionRange::new(
                PhpVersion::new(8, 1),
                PhpVersion::new(8, 5),
            ));
        let names = filtered_names(&db, stubs, configuration);
        let executed = db.take_executed();
        assert!(
            executed
                .iter()
                .any(|entry| entry.starts_with("stubs_in_range")),
            "expected a recomputation, saw {executed:?}",
        );
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn editing_a_source_file_leaves_the_filtered_view_untouched() {
        // The invalidation-scope assertion for this part: file edits
        // (the low-durability input) never touch the stub derivation.
        let mut db = TestDatabase::default();
        let stubs = sample_input(&db);
        let configuration = configuration(&db, (8, 1), (8, 5));
        let file = SourceFile::new(&db, FileId::new(0), b"<?php echo 1;".to_vec());
        let _ = stubs_in_range(&db, stubs, configuration);
        let _ = celerrate_db::parse(&db, file);
        db.take_executed();

        file.set_bytes(&mut db).to(b"<?php echo 2;".to_vec());
        let _ = celerrate_db::parse(&db, file);
        let _ = stubs_in_range(&db, stubs, configuration);
        let executed = db.take_executed();
        assert!(
            executed.iter().any(|entry| entry.starts_with("parse")),
            "the edit reparses, saw {executed:?}",
        );
        assert!(
            !executed
                .iter()
                .any(|entry| entry.starts_with("stubs_in_range")),
            "the stub view must not recompute on a file edit, saw {executed:?}",
        );
    }
}
