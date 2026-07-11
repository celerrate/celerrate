//! The statically named references of one file, collected from its own
//! syntax tree — the design's deliberate boundary exception: a per-file
//! output may read its own tree, never another file's. Dynamic
//! references (`new $class`, call-by-string) are out of scope by
//! documented engine semantics, as are the relative class names
//! `self`, `parent`, `static`.

use celerrate_source::TextRange;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

use crate::symbols::SymbolSpace;

/// One statically named reference, as typed, with its resolution
/// context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub written: String,
    pub space: SymbolSpace,
    pub namespace: String,
    pub range: TextRange,
}

/// Every statically named reference of the file, in tree order.
pub fn collect_references(root: &SyntaxNode) -> Vec<Reference> {
    let mut references = Vec::new();
    let mut namespace = String::new();
    collect(root, &mut namespace, &mut references);
    references
}

fn collect(node: &SyntaxNode, namespace: &mut String, references: &mut Vec<Reference>) {
    for child in node.children() {
        if child.kind() == SyntaxKind::NamespaceDeclaration {
            let Some(declaration) = ast::NamespaceDeclaration::cast(child.clone()) else {
                continue;
            };
            let declared = declaration
                .name()
                .map(|name| name.text())
                .unwrap_or_default();
            match declaration.block() {
                Some(block) => {
                    let mut inner = declared;
                    collect(block.syntax(), &mut inner, references);
                }
                None => *namespace = declared,
            }
            continue;
        }
        visit(&child, namespace, references);
        collect(&child, namespace, references);
    }
}

/// The reference sites of one node; the descent happens in `collect`.
fn visit(node: &SyntaxNode, namespace: &str, references: &mut Vec<Reference>) {
    match node.kind() {
        SyntaxKind::NewExpression => {
            if let Some(expression) = ast::NewExpression::cast(node.clone())
                && let Some(name) = expression.name()
            {
                push_class_like(&name, namespace, references);
            }
        }
        SyntaxKind::ExtendsClause => {
            if let Some(clause) = ast::ExtendsClause::cast(node.clone()) {
                for name in clause.names() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::ImplementsClause => {
            if let Some(clause) = ast::ImplementsClause::cast(node.clone()) {
                for name in clause.names() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::TraitUseClause => {
            if let Some(clause) = ast::TraitUseClause::cast(node.clone()) {
                for name in clause.names() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::CatchClause => {
            if let Some(clause) = ast::CatchClause::cast(node.clone()) {
                for name in clause.names() {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        SyntaxKind::Attribute => {
            if let Some(attribute) = ast::Attribute::cast(node.clone())
                && let Some(name) = attribute.name()
            {
                push_class_like(&name, namespace, references);
            }
        }
        SyntaxKind::NamedType => {
            if let Some(named) = ast::NamedType::cast(node.clone())
                && let Some(ast::NamedTypeName::Name(name)) = named.name_or_keyword()
            {
                let written = name.text();
                if !is_built_in_type_name(&written) {
                    push_class_like(&name, namespace, references);
                }
            }
        }
        _ => {}
    }
}

fn push_class_like(name: &ast::Name, namespace: &str, references: &mut Vec<Reference>) {
    let written = name.text();
    if written.is_empty() || is_relative_class_name(&written) {
        return;
    }
    references.push(Reference {
        written,
        space: SymbolSpace::ClassLike,
        namespace: namespace.to_owned(),
        range: name.syntax().text_range(),
    });
}

/// `self`, `parent`, `static`: resolved against the enclosing class,
/// never against the symbol index.
fn is_relative_class_name(written: &str) -> bool {
    ["self", "parent", "static"]
        .iter()
        .any(|relative| written.eq_ignore_ascii_case(relative))
}

/// Type names PHP resolves without the symbol table. `array`,
/// `callable`, and `static` are lexer keywords and never surface as
/// `Name` nodes; the rest lex as plain identifiers.
fn is_built_in_type_name(written: &str) -> bool {
    [
        "bool", "false", "float", "int", "iterable", "mixed", "never", "null", "object", "parent",
        "self", "string", "true", "void",
    ]
    .iter()
    .any(|built_in| written.eq_ignore_ascii_case(built_in))
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
    use celerrate_syntax::parse;

    fn collected(source: &str) -> Vec<(String, SymbolSpace, String)> {
        collect_references(&parse(source).tree())
            .into_iter()
            .map(|reference| (reference.written, reference.space, reference.namespace))
            .collect()
    }

    fn class_like(written: &str, namespace: &str) -> (String, SymbolSpace, String) {
        (
            written.to_owned(),
            SymbolSpace::ClassLike,
            namespace.to_owned(),
        )
    }

    #[test]
    fn a_new_expression_references_its_class() {
        assert_eq!(
            collected("<?php namespace App; $x = new Client();"),
            vec![class_like("Client", "App")],
        );
    }

    #[test]
    fn inheritance_clauses_reference_every_name() {
        assert_eq!(
            collected("<?php class A extends B implements C, D {}"),
            vec![
                class_like("B", ""),
                class_like("C", ""),
                class_like("D", "")
            ],
        );
    }

    #[test]
    fn trait_use_catch_and_attributes_reference_classes() {
        assert_eq!(
            collected(
                "<?php #[Route] class A { use Loggable; } \
                 try {} catch (NotFound|\\Lib\\Denied $error) {}",
            ),
            vec![
                class_like("Route", ""),
                class_like("Loggable", ""),
                class_like("NotFound", ""),
                class_like("\\Lib\\Denied", ""),
            ],
        );
    }

    #[test]
    fn relative_class_names_are_not_references() {
        assert_eq!(
            collected(
                "<?php class A extends B {
                    function f() { new self(); new static(); return new parent(); }
                }",
            ),
            vec![class_like("B", "")],
        );
    }

    #[test]
    fn brace_form_namespaces_scope_their_references() {
        assert_eq!(
            collected("<?php namespace A { new X(); } namespace B { new Y(); }"),
            vec![class_like("X", "A"), class_like("Y", "B")],
        );
    }

    #[test]
    fn anonymous_class_inheritance_is_still_referenced() {
        assert_eq!(
            collected("<?php $x = new class extends Base {};"),
            vec![class_like("Base", "")],
        );
    }

    #[test]
    fn wreckage_names_are_skipped() {
        // Error-recovery trees never produce empty written names.
        for reference in collect_references(&parse("<?php new ; class extends {}").tree()) {
            assert!(!reference.written.is_empty());
        }
    }

    #[test]
    fn type_positions_reference_class_names() {
        assert_eq!(
            collected(
                "<?php function f(?Request $request, int|string $count, (Left&Right)|null $union): Response {}
                 class C { public UserId $id; }",
            ),
            vec![
                class_like("Request", ""),
                class_like("Left", ""),
                class_like("Right", ""),
                class_like("Response", ""),
                class_like("UserId", ""),
            ],
        );
    }

    #[test]
    fn built_in_type_names_are_not_references() {
        assert_eq!(
            collected(
                "<?php function f(int $a, float $b, string $c, bool $d, mixed $e,
                     iterable $f, object $g, array $h, callable $i, self $j,
                     null|false|true $k): void {}
                 enum Suit: string {}",
            ),
            vec![],
        );
    }
}
