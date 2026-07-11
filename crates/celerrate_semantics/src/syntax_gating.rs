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
        match node.kind() {
            SyntaxKind::ClassDeclaration => {
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
            SyntaxKind::ParenthesizedType => uses.push(GatedUse {
                label: "parenthesized (DNF) type",
                required: PhpVersion::new(8, 2),
                range: node.text_range(),
            }),
            SyntaxKind::ConstantDeclaration => {
                if let Some(declaration) = ast::ConstantDeclaration::cast(node)
                    && let Some(constant_type) = declaration.ty()
                {
                    uses.push(GatedUse {
                        label: "typed constant",
                        required: PhpVersion::new(8, 3),
                        range: constant_type.syntax().text_range(),
                    });
                }
            }
            SyntaxKind::ScopedAccessExpression => {
                if let Some(access) = ast::ScopedAccessExpression::cast(node)
                    && let Some(member) = access.member_name()
                {
                    let opens_with_brace = member
                        .syntax()
                        .children_with_tokens()
                        .find(|element| {
                            element
                                .as_token()
                                .is_none_or(|token| !token.kind().is_trivia())
                        })
                        .and_then(|element| element.into_token())
                        .is_some_and(|token| token.kind() == SyntaxKind::OpenBrace);
                    if opens_with_brace {
                        uses.push(GatedUse {
                            label: "dynamic class constant fetch",
                            required: PhpVersion::new(8, 3),
                            range: member.syntax().text_range(),
                        });
                    }
                }
            }
            SyntaxKind::PropertyHookList => uses.push(GatedUse {
                label: "property hooks",
                required: PhpVersion::new(8, 4),
                range: node.text_range(),
            }),
            SyntaxKind::PropertyDeclaration | SyntaxKind::Parameter => {
                if let Some(range) = asymmetric_visibility(&node) {
                    uses.push(GatedUse {
                        label: "asymmetric visibility",
                        required: PhpVersion::new(8, 4),
                        range,
                    });
                }
            }
            SyntaxKind::BinaryExpression => {
                let operator = ast::BinaryExpression::cast(node)
                    .and_then(|binary| binary.operator_token())
                    .filter(|token| token.kind() == SyntaxKind::PipeGreater);
                if let Some(operator) = operator {
                    uses.push(GatedUse {
                        label: "pipe operator",
                        required: PhpVersion::new(8, 5),
                        range: operator.text_range(),
                    });
                }
            }
            SyntaxKind::CloneExpression => {
                if let Some(clone) = ast::CloneExpression::cast(node)
                    && let Some(arguments) = clone.argument_list()
                {
                    let is_clone_with = arguments.arguments().count() >= 2
                        || arguments
                            .arguments()
                            .any(|argument| argument.label_token().is_some());
                    if is_clone_with {
                        uses.push(GatedUse {
                            label: "clone with arguments",
                            required: PhpVersion::new(8, 5),
                            range: arguments.syntax().text_range(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    uses
}

/// A visibility token directly followed by `(`: the 8.4
/// `private(set)` form, parsed as flat tokens.
fn asymmetric_visibility(node: &SyntaxNode) -> Option<TextRange> {
    let tokens: Vec<_> = node
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !token.kind().is_trivia())
        .collect();
    tokens
        .iter()
        .zip(tokens.iter().skip(1))
        .find(|(first, second)| {
            matches!(
                first.kind(),
                SyntaxKind::Public | SyntaxKind::Protected | SyntaxKind::Private
            ) && second.kind() == SyntaxKind::OpenParenthesis
        })
        .map(|(first, _)| first.text_range())
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

    #[test]
    fn each_gated_construct_reports_its_version() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "<?php function f((Left&Right)|null $x) {}",
                "parenthesized (DNF) type",
                "8.2",
            ),
            (
                "<?php class C { const int LIMIT = 1; }",
                "typed constant",
                "8.3",
            ),
            (
                "<?php $x = Config::{$name};",
                "dynamic class constant fetch",
                "8.3",
            ),
            (
                "<?php class C { public string $p { get => 'v'; } }",
                "property hooks",
                "8.4",
            ),
            (
                "<?php class C { public private(set) string $p; }",
                "asymmetric visibility",
                "8.4",
            ),
            ("<?php $y = $x |> strlen(...);", "pipe operator", "8.5"),
            (
                "<?php $c = clone($point, ['x' => 1]);",
                "clone with arguments",
                "8.5",
            ),
        ];
        for (source, label, version) in cases {
            let diagnostics = gated(source, PhpVersion::new(8, 1));
            let expected = format!(
                "`{label}` requires PHP {version}, but the project's minimum PHP version is 8.1",
            );
            assert!(
                diagnostics.iter().any(|d| d.message == expected),
                "{source}: {diagnostics:?}",
            );
            assert_eq!(gated(source, PhpVersion::new(8, 5)), vec![], "{source}");
        }
    }

    #[test]
    fn a_static_property_access_is_not_a_dynamic_constant_fetch() {
        assert_eq!(
            gated("<?php $x = Config::$value;", PhpVersion::new(8, 1)),
            vec![]
        );
    }

    #[test]
    fn a_single_positional_clone_argument_is_not_gated() {
        // Pre-8.5 PHP reads `clone($x)` as `clone` of a parenthesized
        // expression; gating it would be a false positive.
        assert_eq!(
            gated("<?php $c = clone($point);", PhpVersion::new(8, 1)),
            vec![]
        );
        let named = gated("<?php $c = clone(object: $point);", PhpVersion::new(8, 1));
        assert_eq!(named.len(), 1);
    }

    #[test]
    fn an_identifier_class_constant_is_not_a_dynamic_fetch() {
        assert_eq!(
            gated("<?php $x = Config::VERSION;", PhpVersion::new(8, 1)),
            vec![]
        );
    }

    #[test]
    fn a_promoted_property_with_asymmetric_visibility_is_gated() {
        let source =
            "<?php class C { public function __construct(public private(set) string $x) {} }";
        let diagnostics = gated(source, PhpVersion::new(8, 1));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message,
            "`asymmetric visibility` requires PHP 8.4, but the project's minimum PHP version is 8.1",
        );
        assert_eq!(gated(source, PhpVersion::new(8, 5)), vec![]);
    }
}
