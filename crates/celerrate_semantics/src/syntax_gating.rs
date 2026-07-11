//! The syntax version-gating family: a construct-to-minimum-version
//! table over the file's own typed AST, checked against the range
//! minimum. This is the design's deliberate boundary exception: an
//! output strictly local to the file may read its own tree. The parser
//! always parses the newest grammar; using a construct the range
//! minimum predates is a semantic diagnostic, never a parse failure.

use celerrate_db::SourceFile;
use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
use celerrate_project::{PhpVersion, ProjectConfiguration};
use celerrate_source::TextRange;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

/// A syntax construct newer than the range minimum.
pub const SYNTAX_NOT_AVAILABLE: DiagnosticId = DiagnosticId::new("CEL0024");

/// One use of a version-gated construct.
struct GatedUse {
    label: &'static str,
    required: PhpVersion,
    range: TextRange,
}

/// The per-file syntax gating diagnostics.
#[salsa::tracked(returns(ref))]
pub fn syntax_version_diagnostics(
    db: &dyn salsa::Database,
    file: SourceFile,
    configuration: ProjectConfiguration,
) -> Vec<Diagnostic> {
    let minimum = configuration.php_version_range(db).minimum;
    let file_id = file.file_id(db);
    let root = celerrate_db::parse(db, file).tree();
    let mut diagnostics: Vec<Diagnostic> = gated_uses(&root)
        .into_iter()
        .filter(|gated| gated.required > minimum)
        .map(|gated| Diagnostic {
            id: SYNTAX_NOT_AVAILABLE,
            severity: Severity::Error,
            file: file_id,
            range: gated.range,
            message: format!(
                "`{}` requires PHP {}, but the project's minimum PHP version is {minimum}",
                gated.label, gated.required,
            ),
        })
        .collect();
    diagnostics.sort();
    diagnostics
}

/// Every gated-construct use in the file, in tree order. One match arm
/// per construct: growing the table is adding an arm.
fn gated_uses(root: &SyntaxNode) -> Vec<GatedUse> {
    let mut uses = Vec::new();
    for node in root.descendants() {
        if let Some(declaration) = ast::ClassDeclaration::cast(node)
            && let Some(readonly) = declaration
                .modifiers()
                .find(|token| token.kind() == SyntaxKind::Readonly)
        {
            uses.push(GatedUse {
                label: "readonly class",
                required: PhpVersion::new(8, 2),
                range: readonly.text_range(),
            });
        }
    }
    uses
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;

    fn gated(source: &str, minimum: PhpVersion) -> Vec<Diagnostic> {
        let db = TestDatabase::default();
        let file = SourceFile::new(&db, FileId::new(0), source.as_bytes().to_vec());
        let configuration =
            ProjectConfiguration::builder(PhpVersionRange::new(minimum, PhpVersion::new(8, 5)))
                .durability(salsa::Durability::MEDIUM)
                .new(&db);
        syntax_version_diagnostics(&db, file, configuration).clone()
    }

    #[test]
    fn a_readonly_class_is_gated_below_its_version() {
        let diagnostics = gated("<?php readonly class Point {}", PhpVersion::new(8, 1));
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostic.id, SYNTAX_NOT_AVAILABLE);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(
            diagnostic.message,
            "`readonly class` requires PHP 8.2, but the project's minimum PHP version is 8.1",
        );
    }

    #[test]
    fn a_construct_within_the_range_minimum_is_silent() {
        assert_eq!(
            gated("<?php readonly class Point {}", PhpVersion::new(8, 2)),
            vec![]
        );
    }

    #[test]
    fn a_readonly_property_is_not_a_readonly_class() {
        assert_eq!(
            gated(
                "<?php class Point { public readonly int $x; }",
                PhpVersion::new(8, 1),
            ),
            vec![],
        );
    }
}
