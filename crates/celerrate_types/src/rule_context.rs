use celerrate_semantics::AstId;

/// The typed-body-phase context, owned by this crate (design section
/// 4). One context per checked body — the per-body tracked tier's
/// unit. Part 4's family migrations enumerate its real facade methods
/// (body IR interrogation, inferred types, membership and
/// assignability questions), each recording its consulted classes
/// structurally; until then it carries only the body's identity.
pub struct TypedBodyContext<'db> {
    _database: &'db dyn salsa::Database,
    body: AstId,
}

impl TypedBodyContext<'_> {
    /// The identity of the body under check.
    pub fn body(&self) -> AstId {
        self.body
    }
}

/// Engine construction seam, database-gated like `semantic_context`.
pub fn typed_body_context<'db>(db: &'db dyn salsa::Database, body: AstId) -> TypedBodyContext<'db> {
    TypedBodyContext {
        _database: db,
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

    use celerrate_db::testing::TestDatabase;
    use celerrate_semantics::AstId;
    use celerrate_source::FileId;

    use crate::rule_context::typed_body_context;

    #[test]
    fn the_typed_body_context_exposes_the_bodys_identity_and_never_the_database() {
        let db = TestDatabase::default();
        let ast_id = AstId {
            file: FileId::new(0),
            index: 0,
        };
        let context = typed_body_context(&db, ast_id);
        assert_eq!(context.body(), ast_id);
    }
}
