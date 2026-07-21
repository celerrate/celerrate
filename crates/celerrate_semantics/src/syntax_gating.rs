//! The syntax version-gating walk: a construct-to-minimum-version table
//! over the file's own typed AST. This is the design's deliberate
//! boundary exception: an output strictly local to the file may read
//! its own tree. The parser always parses the newest grammar; using a
//! construct the range minimum predates is a semantic diagnostic, never
//! a parse failure. The walk stays here; comparing each use against the
//! range minimum and constructing the diagnostic now belongs to the
//! rule framework's syntax phase
//! (`celerrate_rules::rules::syntax_version_gating`).

use celerrate_db::SourceFile;
use celerrate_project::PhpVersion;
use celerrate_source::TextRange;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

/// One use of a version-gated construct, in tree order: the outcome
/// the syntax-version-gating rule consumes. The walk stays below; the
/// rule turns outcomes into diagnostics (design section 2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GatedSyntaxUse {
    pub label: &'static str,
    pub required: PhpVersion,
    pub range: TextRange,
}

/// Every gated-construct use in the file, in tree order.
/// Version-range independent on purpose: a configuration change
/// re-filters without re-walking.
#[salsa::tracked(returns(ref))]
pub fn gated_syntax_uses(db: &dyn salsa::Database, file: SourceFile) -> Vec<GatedSyntaxUse> {
    let root = celerrate_db::parse(db, file).tree();
    collect_gated_uses(&root)
}

/// Every gated-construct use in the file, in tree order. One match arm
/// per construct: growing the table is adding an arm.
fn collect_gated_uses(root: &SyntaxNode) -> Vec<GatedSyntaxUse> {
    let mut uses = Vec::new();
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::ClassDeclaration => {
                if let Some(declaration) = ast::ClassDeclaration::cast(node)
                    && let Some(readonly) = declaration
                        .modifiers()
                        .find(|token| token.kind() == SyntaxKind::Readonly)
                {
                    uses.push(GatedSyntaxUse {
                        label: "readonly class",
                        required: PhpVersion::new(8, 2),
                        range: readonly.text_range(),
                    });
                }
            }
            SyntaxKind::ParenthesizedType => uses.push(GatedSyntaxUse {
                label: "parenthesized (DNF) type",
                required: PhpVersion::new(8, 2),
                range: node.text_range(),
            }),
            SyntaxKind::ConstantDeclaration => {
                if let Some(declaration) = ast::ConstantDeclaration::cast(node)
                    && let Some(constant_type) = declaration.ty()
                {
                    uses.push(GatedSyntaxUse {
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
                        uses.push(GatedSyntaxUse {
                            label: "dynamic class constant fetch",
                            required: PhpVersion::new(8, 3),
                            range: member.syntax().text_range(),
                        });
                    }
                }
            }
            SyntaxKind::PropertyHookList => uses.push(GatedSyntaxUse {
                label: "property hooks",
                required: PhpVersion::new(8, 4),
                range: node.text_range(),
            }),
            SyntaxKind::PropertyDeclaration | SyntaxKind::Parameter => {
                if let Some(range) = asymmetric_visibility(&node) {
                    uses.push(GatedSyntaxUse {
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
                    uses.push(GatedSyntaxUse {
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
                        uses.push(GatedSyntaxUse {
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
    use celerrate_source::FileId;

    /// The walk is version-range independent by construction: it reports
    /// every gated use in tree order, and the range comparison is the
    /// rule framework's job (`celerrate_rules::rules::syntax_version_gating`,
    /// where the filtering and message behaviors are now tested).
    #[test]
    fn the_walk_reports_every_gated_use_regardless_of_the_version_range() {
        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php readonly class Point { public const int X = 1; }".to_vec(),
        );
        let uses = gated_syntax_uses(&db, file);
        let labels: Vec<&str> = uses.iter().map(|gated| gated.label).collect();
        assert_eq!(labels, vec!["readonly class", "typed constant"]);
        assert_eq!(uses[0].required, PhpVersion::new(8, 2));
    }
}
