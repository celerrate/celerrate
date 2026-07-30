use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{PhpVersionRange, ProjectConfiguration};
use celerrate_source::FileId;
use celerrate_stubs::StubIndexInput;

use crate::reference_checks::{ReferenceOutcome, reference_resolutions};

/// The semantic-phase context, owned by this crate.
/// Sealed: private database, delegating methods, no salsa vocabulary
/// rule-side. The surface matches what the shipped semantic rules
/// consume: `reference_resolutions` for the unknown-symbol family and
/// `php_version_range` for the symbol-version-gating one (the YAGNI
/// criterion); the symbol index arrives with its first client.
pub struct SemanticContext<'db> {
    db: &'db dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

impl<'db> SemanticContext<'db> {
    /// The checked file's identity.
    pub fn file(&self) -> FileId {
        self.file.file_id(self.db)
    }

    /// Every statically named reference's resolution outcome, in walk
    /// order. Delegates to the same memoized walk that co-produces the
    /// cache's revalidation records, so consulting cannot drift from
    /// recording.
    pub fn reference_resolutions(&self) -> &'db [ReferenceOutcome] {
        reference_resolutions(
            self.db,
            self.file,
            self.files,
            self.stubs,
            self.configuration,
        )
    }

    /// The project's supported PHP version range.
    pub fn php_version_range(&self) -> PhpVersionRange {
        self.configuration.php_version_range(self.db)
    }
}

/// Engine construction seam. Public but database-gated: the facade
/// never re-exports salsa nor hands out a database, so a plugin can
/// neither name nor supply the argument (the `testing_type_context`
/// precedent).
pub fn semantic_context<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> SemanticContext<'db> {
    SemanticContext {
        db,
        file,
        files,
        stubs,
        configuration,
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

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use crate::reference_checks::ResolutionOutcome;
    use crate::rule_context::semantic_context;

    #[test]
    fn the_semantic_context_exposes_the_files_identity_and_never_the_database() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php new Missing();".to_vec());
        let files = AnalyzedFileSet::new(&db, vec![file]);
        let stubs = StubIndexInput::builder(StubIndex::default())
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let configuration = ProjectConfiguration::builder(range)
            .durability(salsa::Durability::MEDIUM)
            .new(&db);
        let context = semantic_context(&db, file, files, stubs, configuration);
        assert_eq!(context.file(), FileId::new(0));
        let outcomes = context.reference_resolutions();
        assert_eq!(outcomes.len(), 1, "{outcomes:?}");
        assert_eq!(outcomes[0].written, "Missing");
        assert_eq!(outcomes[0].resolution, ResolutionOutcome::Unresolved);
        assert_eq!(context.php_version_range(), range);
        assert_eq!(context.php_version_range().minimum, PhpVersion::new(8, 1));
        assert_eq!(context.php_version_range().maximum, PhpVersion::new(8, 5));
    }
}
