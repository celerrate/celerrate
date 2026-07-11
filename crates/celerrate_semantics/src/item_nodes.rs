//! The shared traversal defining "the declaration nodes" of one file.
//! `AstIdMap` and the item tree both consume it, so their numbering
//! agrees by construction: preorder tree order, the enclosing namespace
//! tracked as walk state.
//!
//! The walk descends into control-flow blocks and function bodies (a
//! symbol declared behind an `if (!function_exists(...))` guard counts
//! as declared) but never into a `MemberList`: members are not items in
//! this sub-project, so class constants and declarations nested inside
//! method bodies are invisible here. Nameless declarations (anonymous
//! classes, error-recovery wreckage) carry no top-level identity and
//! are skipped.

use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

/// One declaration node with its enclosing namespace (`""` is global).
pub(crate) struct ItemNode {
    pub(crate) node: SyntaxNode,
    pub(crate) namespace: String,
}

/// Every declaration node of the file, in tree order.
pub(crate) fn item_nodes(root: &SyntaxNode) -> Vec<ItemNode> {
    let mut items = Vec::new();
    let mut namespace = String::new();
    collect(root, &mut namespace, &mut items);
    items
}

fn collect(node: &SyntaxNode, namespace: &mut String, items: &mut Vec<ItemNode>) {
    for child in node.children() {
        match child.kind() {
            // Members are not items: class constants and declarations
            // nested inside method bodies stay invisible.
            SyntaxKind::MemberList => {}
            SyntaxKind::NamespaceDeclaration => {
                items.push(ItemNode {
                    node: child.clone(),
                    namespace: namespace.clone(),
                });
                let Some(declaration) = ast::NamespaceDeclaration::cast(child.clone()) else {
                    continue;
                };
                let declared = declaration
                    .name()
                    .map(|name| name.text())
                    .unwrap_or_default();
                match declaration.block() {
                    // Brace form: the name scopes the block, nothing else.
                    Some(block) => {
                        let mut inner = declared;
                        collect(block.syntax(), &mut inner, items);
                    }
                    // Statement form: the name applies to what follows.
                    None => *namespace = declared,
                }
            }
            _ => {
                if is_item(&child) {
                    items.push(ItemNode {
                        node: child.clone(),
                        namespace: namespace.clone(),
                    });
                }
                collect(&child, namespace, items);
            }
        }
    }
}

/// Whether one node is a declaration node. Class-likes and functions
/// must be named: anonymous classes and error-recovery wreckage carry
/// no top-level identity.
fn is_item(node: &SyntaxNode) -> bool {
    match node.kind() {
        SyntaxKind::UseDeclaration | SyntaxKind::ConstantDeclaration => true,
        SyntaxKind::ClassDeclaration => ast::ClassDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        SyntaxKind::InterfaceDeclaration => ast::InterfaceDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        SyntaxKind::TraitDeclaration => ast::TraitDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        SyntaxKind::EnumDeclaration => ast::EnumDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        SyntaxKind::FunctionDeclaration => ast::FunctionDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        _ => false,
    }
}
