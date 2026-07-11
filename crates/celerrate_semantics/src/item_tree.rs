//! The per-file item tree: the `Eq`-comparable, deterministically
//! ordered projection of one file's declarations. It carries no ranges
//! and no offsets — a body, comment, or whitespace edit produces an
//! identical value, salsa backdates it, and nothing downstream re-runs.
//! That equality is the invalidation boundary of the engine.

use celerrate_source::FileId;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::ast_id::AstId;
use crate::item_nodes::{ItemNode, item_nodes};

/// The kind of a declared symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclarationKind {
    Class,
    Interface,
    Trait,
    Enum,
    Function,
    Constant,
}

/// One declared symbol: original spelling, enclosing namespace (`""`
/// is global), stable identity, and the unresolved inheritance names
/// exactly as written (sub-project 3 consumes them; they cost one
/// field now).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Declaration {
    pub kind: DeclarationKind,
    pub name: String,
    pub namespace: String,
    pub ast_id: AstId,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub trait_uses: Vec<String>,
}

/// What one `use` clause imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportKind {
    Class,
    Function,
    Constant,
}

/// One expanded `use` import: group forms are flattened, the target is
/// the written absolute name (leading backslash trimmed), and the
/// alias is the explicit one or the target's last segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UseImport {
    pub kind: ImportKind,
    pub target: String,
    pub alias: String,
    pub namespace: String,
    pub ast_id: AstId,
}

/// The projection of one file's declarations and imports, in tree
/// order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemTree {
    pub declarations: Vec<Declaration>,
    pub imports: Vec<UseImport>,
}

impl ItemTree {
    /// Projects one file's syntax tree. Positions in the shared
    /// declaration-node traversal are the `AstId` indexes, so this
    /// numbering and [`crate::AstIdMap`]'s agree by construction.
    pub fn from_root(file: FileId, root: &SyntaxNode) -> Self {
        let mut tree = ItemTree::default();
        for (position, item) in item_nodes(root).into_iter().enumerate() {
            let Ok(index) = u32::try_from(position) else {
                break;
            };
            lower(&item, AstId { file, index }, &mut tree);
        }
        tree
    }
}

/// The unresolved inheritance names of one class-like declaration.
struct Inheritance {
    extends: Vec<String>,
    implements: Vec<String>,
    trait_uses: Vec<String>,
}

impl Inheritance {
    const NONE: Inheritance = Inheritance {
        extends: Vec::new(),
        implements: Vec::new(),
        trait_uses: Vec::new(),
    };
}

fn lower(item: &ItemNode, ast_id: AstId, tree: &mut ItemTree) {
    match item.node.kind() {
        SyntaxKind::ClassDeclaration => {
            if let Some(declaration) = ast::ClassDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Class,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::InterfaceDeclaration => {
            if let Some(declaration) = ast::InterfaceDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Interface,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::TraitDeclaration => {
            if let Some(declaration) = ast::TraitDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Trait,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::EnumDeclaration => {
            if let Some(declaration) = ast::EnumDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Enum,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::FunctionDeclaration => {
            if let Some(declaration) = ast::FunctionDeclaration::cast(item.node.clone()) {
                push_declaration(
                    tree,
                    item,
                    ast_id,
                    DeclarationKind::Function,
                    declaration.name_token(),
                    Inheritance::NONE,
                );
            }
        }
        SyntaxKind::ConstantDeclaration => {
            if let Some(declaration) = ast::ConstantDeclaration::cast(item.node.clone()) {
                for element in declaration.constant_elements() {
                    push_declaration(
                        tree,
                        item,
                        ast_id,
                        DeclarationKind::Constant,
                        element.name_token(),
                        Inheritance::NONE,
                    );
                }
            }
        }
        // Use declarations expand into imports (a later task);
        // namespace declarations carry no projection of their own.
        _ => {}
    }
}

fn push_declaration(
    tree: &mut ItemTree,
    item: &ItemNode,
    ast_id: AstId,
    kind: DeclarationKind,
    name_token: Option<SyntaxToken>,
    inheritance: Inheritance,
) {
    let Some(name_token) = name_token else { return };
    tree.declarations.push(Declaration {
        kind,
        name: name_token.text().to_owned(),
        namespace: item.namespace.clone(),
        ast_id,
        extends: inheritance.extends,
        implements: inheritance.implements,
        trait_uses: inheritance.trait_uses,
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::FileId;

    use super::{DeclarationKind, ItemTree};
    use crate::ast_id::AstId;

    fn tree_of(source: &str) -> ItemTree {
        ItemTree::from_root(FileId::new(0), &celerrate_syntax::parse(source).tree())
    }

    fn declared(source: &str) -> Vec<(DeclarationKind, String, String)> {
        tree_of(source)
            .declarations
            .iter()
            .map(|declaration| {
                (
                    declaration.kind,
                    declaration.name.clone(),
                    declaration.namespace.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn every_declaration_kind_is_projected() {
        assert_eq!(
            declared(
                "<?php\n\
                 class Service {}\n\
                 interface Contract {}\n\
                 trait Helper {}\n\
                 enum Suit {}\n\
                 function greet() {}\n\
                 const LIMIT = 1;\n",
            ),
            vec![
                (DeclarationKind::Class, "Service".to_owned(), String::new()),
                (
                    DeclarationKind::Interface,
                    "Contract".to_owned(),
                    String::new()
                ),
                (DeclarationKind::Trait, "Helper".to_owned(), String::new()),
                (DeclarationKind::Enum, "Suit".to_owned(), String::new()),
                (DeclarationKind::Function, "greet".to_owned(), String::new()),
                (DeclarationKind::Constant, "LIMIT".to_owned(), String::new()),
            ],
        );
    }

    #[test]
    fn a_statement_form_namespace_scopes_everything_after_it() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace First;\n\
                 function one() {}\n\
                 namespace Second;\n\
                 function two() {}\n",
            ),
            vec![
                (
                    DeclarationKind::Function,
                    "one".to_owned(),
                    "First".to_owned()
                ),
                (
                    DeclarationKind::Function,
                    "two".to_owned(),
                    "Second".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn brace_form_namespaces_scope_their_block_only() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace Ds { class Vector {} }\n\
                 namespace { function outside() {} }\n",
            ),
            vec![
                (DeclarationKind::Class, "Vector".to_owned(), "Ds".to_owned()),
                (
                    DeclarationKind::Function,
                    "outside".to_owned(),
                    String::new()
                ),
            ],
        );
    }

    #[test]
    fn guarded_and_nested_declarations_are_projected() {
        assert_eq!(
            declared(
                "<?php\n\
                 namespace App;\n\
                 if (!function_exists('greet')) { function greet() {} }\n\
                 function outer() { function inner() {} }\n",
            ),
            vec![
                (
                    DeclarationKind::Function,
                    "greet".to_owned(),
                    "App".to_owned()
                ),
                (
                    DeclarationKind::Function,
                    "outer".to_owned(),
                    "App".to_owned()
                ),
                (
                    DeclarationKind::Function,
                    "inner".to_owned(),
                    "App".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn members_are_not_projected() {
        assert_eq!(
            declared(
                "<?php class A { const B = 1; public $property; public function method() {} }",
            ),
            vec![(DeclarationKind::Class, "A".to_owned(), String::new())],
        );
    }

    #[test]
    fn anonymous_classes_are_not_projected() {
        assert_eq!(
            declared("<?php $instance = new class {}; class Named {}"),
            vec![(DeclarationKind::Class, "Named".to_owned(), String::new())],
        );
    }

    #[test]
    fn a_grouped_constant_declaration_projects_one_entry_per_element() {
        let tree = tree_of("<?php const A = 1, B = 2;");
        let names: Vec<&str> = tree
            .declarations
            .iter()
            .map(|declaration| declaration.name.as_str())
            .collect();
        assert_eq!(names, vec!["A", "B"]);
        // Both elements share the declaration node's identity.
        assert_eq!(
            tree.declarations.first().unwrap().ast_id,
            tree.declarations.last().unwrap().ast_id,
        );
    }

    #[test]
    fn ast_ids_are_the_tree_order_positions_of_the_declaration_nodes() {
        // Numbering: namespace = 0, use = 1, class = 2.
        let tree = ItemTree::from_root(
            FileId::new(7),
            &celerrate_syntax::parse("<?php namespace N; use A; class B {}").tree(),
        );
        assert_eq!(
            tree.declarations
                .first()
                .map(|declaration| declaration.ast_id),
            Some(AstId {
                file: FileId::new(7),
                index: 2,
            }),
        );
    }

    #[test]
    fn original_spelling_is_preserved() {
        // Case folding is the index's concern (part 5), never the tree's.
        assert_eq!(
            declared("<?php class MiXeDcAsE {}"),
            vec![(
                DeclarationKind::Class,
                "MiXeDcAsE".to_owned(),
                String::new()
            )],
        );
    }

    #[test]
    fn malformed_input_projects_what_the_parser_recovered() {
        assert_eq!(
            declared("<?php class Broken { function ok() {}"),
            vec![(DeclarationKind::Class, "Broken".to_owned(), String::new())],
        );
    }

    #[test]
    fn a_body_edit_produces_an_identical_item_tree() {
        // The early-cutoff property, at the value level: no ranges, no
        // offsets, so bodies, comments, and whitespace never show up.
        let before = tree_of("<?php function greet() { return 1; } class After {}");
        let body_edit = tree_of("<?php function greet() { return 2; } class After {}");
        let comment_edit =
            tree_of("<?php // note\nfunction greet() { return 1; }   class After {}");
        assert_eq!(before, body_edit);
        assert_eq!(before, comment_edit);
    }

    #[test]
    fn empty_and_html_only_files_project_nothing() {
        assert_eq!(tree_of("").declarations, Vec::new());
        assert_eq!(tree_of("plain text, no PHP").declarations, Vec::new());
    }
}
