use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_semantics::{AstId, BodyQuery};
use celerrate_stubs::StubIndexInput;

use celerrate_project::ProjectConfiguration;

use crate::checks::TypedVerdict;

/// The typed-body-phase context, owned by this crate (design section
/// 4). One context per checked body, the per-body tracked tier's unit.
/// Sealed on the `InvocationSite` model: private database, delegating
/// methods, no salsa vocabulary rule-side.
pub struct TypedBodyContext<'db> {
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: AstId,
}

impl<'db> TypedBodyContext<'db> {
    /// The identity of the body under check.
    pub fn body(&self) -> AstId {
        self.body
    }

    /// The body's typed outcome records, in walk order: the membership
    /// answers, nullability judgments, and argument judgments the walk
    /// below produced. Delegates to the same memoized per-body query
    /// whose consulted-class set the cache revalidation consumes
    /// (`typed_file_verdicts` aggregates the identical memo), so a
    /// rule cannot consult outcomes without their dependency records
    /// having been produced in the same pass (design section 2).
    pub fn verdicts(&self) -> &'db [TypedVerdict] {
        let body = BodyQuery::new(self.db, self.body);
        &crate::checks::body_typed_verdicts(
            self.db,
            self.files,
            self.stubs,
            self.configuration,
            self.file,
            body,
        )
        .verdicts
    }
}

/// Engine construction seam, database-gated like `semantic_context`.
pub fn typed_body_context<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: AstId,
) -> TypedBodyContext<'db> {
    TypedBodyContext {
        db,
        files,
        stubs,
        configuration,
        file,
        body,
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

    use celerrate_semantics::AstId;
    use celerrate_source::FileId;

    use crate::rule_context::typed_body_context;

    #[test]
    fn the_typed_body_context_exposes_the_bodys_identity_and_never_the_database() {
        let fixture = crate::checks::test_support::fixture(&["<?php function f() { echo 1; }"]);
        let file = crate::checks::test_support::handle_of(&fixture, 0);
        let ast_id = AstId {
            file: FileId::new(0),
            index: 0,
        };
        let context = typed_body_context(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            ast_id,
        );
        assert_eq!(context.body(), ast_id);
    }

    #[test]
    fn the_context_surfaces_the_bodys_outcome_records() {
        let fixture = crate::checks::test_support::fixture(&[r#"<?php
class User { public function save(): void {} }
function f(User $u): void { $u->svae(); }
"#]);
        let file = crate::checks::test_support::handle_of(&fixture, 0);
        // Declaration numbering: `class User` (0), its method (1),
        // then `function f` (2), whose body carries the defect.
        let body = celerrate_semantics::AstId {
            file: celerrate_source::FileId::new(0),
            index: 2,
        };
        let context = typed_body_context(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        );
        let verdicts = context.verdicts();
        assert_eq!(verdicts.len(), 1, "{verdicts:?}");
        assert!(matches!(
            &verdicts[0].kind,
            crate::checks::TypedVerdictKind::UnknownMethod { member, .. } if member == "svae"
        ));
    }
}
