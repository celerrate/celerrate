use celerrate_db::SourceFile;
use celerrate_source::FileId;

/// The semantic-phase context, owned by this crate (design section 4).
/// Sealed: private database, delegating methods, no salsa vocabulary
/// rule-side. Part 4's family migrations enumerate its real facade
/// methods (resolution outcomes, symbol index); until then it carries
/// only plain file identity.
pub struct SemanticContext<'db> {
    db: &'db dyn salsa::Database,
    file: SourceFile,
}

impl SemanticContext<'_> {
    /// The checked file's identity.
    pub fn file(&self) -> FileId {
        self.file.file_id(self.db)
    }
}

/// Engine construction seam. Public but database-gated: the facade
/// never re-exports salsa nor hands out a database, so a plugin can
/// neither name nor supply the argument (the `testing_type_context`
/// precedent).
pub fn semantic_context<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
) -> SemanticContext<'db> {
    SemanticContext { db, file }
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

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_source::FileId;

    use crate::rule_context::semantic_context;

    #[test]
    fn the_semantic_context_exposes_the_files_identity_and_never_the_database() {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), b"<?php".to_vec());
        let context = semantic_context(&db, file);
        assert_eq!(context.file(), FileId::new(0));
    }
}
