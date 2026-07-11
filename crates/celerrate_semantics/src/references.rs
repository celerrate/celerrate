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
        SyntaxKind::NameExpression => {
            if let Some(expression) = ast::NameExpression::cast(node.clone()) {
                visit_name_expression(&expression, namespace, references);
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

/// The role of a `NameExpression`, decided by its parent: the callee
/// of a call is a function reference, the subject of `::` and the
/// right-hand side of `instanceof` are class references, everything
/// else is a constant fetch.
enum NameExpressionRole {
    Callee,
    ClassSubject,
    ConstantFetch,
}

fn visit_name_expression(
    expression: &ast::NameExpression,
    namespace: &str,
    references: &mut Vec<Reference>,
) {
    if expression.static_keyword_token().is_some() {
        return;
    }
    let Some(name) = expression.name() else {
        return;
    };
    match role_of(expression.syntax()) {
        NameExpressionRole::Callee => {
            let written = name.text();
            if written.is_empty() {
                return;
            }
            references.push(Reference {
                written,
                space: SymbolSpace::Function,
                namespace: namespace.to_owned(),
                range: name.syntax().text_range(),
            });
        }
        NameExpressionRole::ClassSubject => push_class_like(&name, namespace, references),
        NameExpressionRole::ConstantFetch => {
            let written = name.text();
            if written.is_empty() || is_language_constant(&written) {
                return;
            }
            references.push(Reference {
                written,
                space: SymbolSpace::Constant,
                namespace: namespace.to_owned(),
                range: name.syntax().text_range(),
            });
        }
    }
}

fn role_of(node: &SyntaxNode) -> NameExpressionRole {
    let Some(parent) = node.parent() else {
        return NameExpressionRole::ConstantFetch;
    };
    match parent.kind() {
        SyntaxKind::CallExpression => {
            let is_callee = ast::CallExpression::cast(parent)
                .and_then(|call| call.callee())
                .is_some_and(|callee| callee.syntax() == node);
            if is_callee {
                NameExpressionRole::Callee
            } else {
                NameExpressionRole::ConstantFetch
            }
        }
        SyntaxKind::ScopedAccessExpression => {
            let is_subject = ast::ScopedAccessExpression::cast(parent)
                .and_then(|access| access.subject())
                .is_some_and(|subject| subject.syntax() == node);
            if is_subject {
                NameExpressionRole::ClassSubject
            } else {
                NameExpressionRole::ConstantFetch
            }
        }
        SyntaxKind::BinaryExpression => {
            let is_instanceof_right_hand_side = ast::BinaryExpression::cast(parent)
                .filter(|binary| {
                    binary
                        .operator_token()
                        .is_some_and(|operator| operator.kind() == SyntaxKind::InstanceOf)
                })
                .and_then(|binary| binary.rhs())
                .is_some_and(|right| right.syntax() == node);
            if is_instanceof_right_hand_side {
                NameExpressionRole::ClassSubject
            } else {
                NameExpressionRole::ConstantFetch
            }
        }
        _ => NameExpressionRole::ConstantFetch,
    }
}

/// `true`, `false`, `null`, and the magic constants: language-defined,
/// never symbol-table lookups. Compared after trimming one leading `\`
/// (`\true` is the same literal), only for single-segment names.
fn is_language_constant(written: &str) -> bool {
    let unqualified = written.strip_prefix('\\').unwrap_or(written);
    if unqualified.contains('\\') {
        return false;
    }
    [
        "true",
        "false",
        "null",
        "__LINE__",
        "__FILE__",
        "__DIR__",
        "__FUNCTION__",
        "__CLASS__",
        "__TRAIT__",
        "__METHOD__",
        "__NAMESPACE__",
        "__PROPERTY__",
    ]
    .iter()
    .any(|constant| unqualified.eq_ignore_ascii_case(constant))
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

    fn function_reference(written: &str, namespace: &str) -> (String, SymbolSpace, String) {
        (
            written.to_owned(),
            SymbolSpace::Function,
            namespace.to_owned(),
        )
    }

    fn constant_reference(written: &str, namespace: &str) -> (String, SymbolSpace, String) {
        (
            written.to_owned(),
            SymbolSpace::Constant,
            namespace.to_owned(),
        )
    }

    #[test]
    fn a_call_references_a_function() {
        assert_eq!(
            collected("<?php namespace App; strlen($x); \\count($y); inner(outer());"),
            vec![
                function_reference("strlen", "App"),
                function_reference("\\count", "App"),
                function_reference("inner", "App"),
                function_reference("outer", "App"),
            ],
        );
    }

    #[test]
    fn a_scoped_subject_references_its_class() {
        assert_eq!(
            collected("<?php Status::Open; Config::class; Client::create(); $x::CONST;"),
            vec![
                class_like("Status", ""),
                class_like("Config", ""),
                class_like("Client", ""),
            ],
        );
    }

    #[test]
    fn an_instanceof_right_hand_side_is_a_class() {
        assert_eq!(
            collected("<?php $ok = $x instanceof Comparable;"),
            vec![class_like("Comparable", "")],
        );
    }

    #[test]
    fn a_bare_name_is_a_constant_reference() {
        assert_eq!(
            collected("<?php $a = PHP_EOL; $b = Config\\LIMIT;"),
            vec![
                constant_reference("PHP_EOL", ""),
                constant_reference("Config\\LIMIT", ""),
            ],
        );
    }

    #[test]
    fn language_and_magic_constants_are_not_references() {
        assert_eq!(
            collected(
                "<?php $a = true; $b = FALSE; $c = null; $d = \\true;
                 $e = __DIR__; $f = __class__; $g = __NAMESPACE__;",
            ),
            vec![],
        );
    }

    #[test]
    fn relative_and_dynamic_subjects_are_not_references() {
        assert_eq!(
            collected("<?php self::f(); parent::g(); static::h(); $x instanceof self;"),
            vec![],
        );
    }
}
