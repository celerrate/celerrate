use celerrate_db::SourceFile;
use celerrate_project::{PhpVersionRange, ProjectConfiguration};
use celerrate_semantics::GatedSyntaxUse;

/// The syntax-phase context. Owned by `celerrate_rules` — its contents
/// span `celerrate_db` and `celerrate_project` with no single domain
/// owner, a deliberate exception to that rule. Sealed on the
/// `InvocationSite` model: the database is private, methods delegate,
/// and no salsa vocabulary appears in any rule-facing signature. The
/// surface is exactly what the shipped syntax rules consume (the
/// `TypeContext` YAGNI criterion); the line index and any generic tree
/// interrogation arrive with their first client.
pub struct SyntaxContext<'db> {
    db: &'db dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
}

impl<'db> SyntaxContext<'db> {
    pub(crate) fn new(
        db: &'db dyn salsa::Database,
        file: SourceFile,
        configuration: ProjectConfiguration,
    ) -> Self {
        Self {
            db,
            file,
            configuration,
        }
    }

    /// The project's supported PHP version range.
    pub fn php_version_range(&self) -> PhpVersionRange {
        self.configuration.php_version_range(self.db)
    }

    /// Every version-gated construct use in the file, in tree order.
    pub fn gated_syntax_uses(&self) -> &'db [GatedSyntaxUse] {
        celerrate_semantics::gated_syntax_uses(self.db, self.file)
    }
}

/// Test-only construction seam, same contract as
/// `testing_type_context`: harmless because it demands a database
/// handle, which the facade never provides.
pub fn testing_syntax_context<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
) -> SyntaxContext<'db> {
    SyntaxContext::new(db, file, configuration)
}

/// One directive with its final match outcome: the union of both
/// halves', composed by the orchestration layer (stored records on a
/// warm hit, the resolution query on a miss).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveOutcome {
    pub directive: celerrate_semantics::ResolvedDirective,
    pub matched: bool,
}

/// The `Reporting` phase context: directives and their match outcomes.
/// Core-only - never re-exported by the facade.
/// Plain outcome data plus two registry questions; no database handle,
/// no tree, no parse: the same context serves the warm path.
pub struct ReportingContext<'run> {
    outcomes: &'run [DirectiveOutcome],
    inactive: &'run std::collections::BTreeSet<celerrate_diagnostics::DiagnosticId>,
}

impl<'run> ReportingContext<'run> {
    pub(crate) fn new(
        outcomes: &'run [DirectiveOutcome],
        inactive: &'run std::collections::BTreeSet<celerrate_diagnostics::DiagnosticId>,
    ) -> Self {
        Self { outcomes, inactive }
    }

    /// Every directive of the file, in resolution order, with its
    /// final match outcome.
    pub fn outcomes(&self) -> &'run [DirectiveOutcome] {
        self.outcomes
    }

    /// Whether the written form names a registered identifier - the
    /// CEL0041 knownness question, asked through the single lookup
    /// seam (`find_identifier`, itself a two-tier lookup).
    pub fn is_known(&self, written: &str) -> bool {
        celerrate_diagnostics::find_identifier(written).is_some()
    }

    /// Whether the written form names an identifier of a rule outside
    /// the active set - the CEL0042 exemption question. Resilience
    /// identifiers are claimed by no rule and are never inactive.
    pub fn is_inactive(&self, written: &str) -> bool {
        celerrate_diagnostics::find_identifier(written)
            .is_some_and(|id| self.inactive.contains(&id))
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

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;

    use crate::context::testing_syntax_context;

    #[test]
    fn the_syntax_context_exposes_outcomes_and_never_the_database() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php readonly class Point {}".to_vec(),
        );
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        let context = testing_syntax_context(&db, file, configuration);
        assert_eq!(context.php_version_range().minimum, PhpVersion::new(8, 1));
        assert_eq!(context.gated_syntax_uses().len(), 1);
        assert_eq!(context.gated_syntax_uses()[0].label, "readonly class");
    }
}
