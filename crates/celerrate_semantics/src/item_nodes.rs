//! The shared traversal defining "the declaration nodes" of one file.
//! `AstIdMap` and both projections consume it, so their numbering
//! agrees by construction: preorder tree order, the enclosing
//! namespace tracked as walk state.
//!
//! The walk descends into control-flow blocks, function bodies, and —
//! since the type-engine sub-project — member lists: methods,
//! properties, constants, and enum cases are numbered items carrying
//! their owning class-like's index, and nameless class-likes
//! (anonymous classes) are numbered items whose position is their
//! synthetic identity. Numbering counts declaration nodes only, so
//! statement edits renumber nothing; adding a member or an anonymous
//! class renumbers later declarations in the file — the recorded,
//! accepted trade-off (spec section 2).
//!
//! A statement-form `namespace Foo;` nested inside a control-flow
//! block (error-recovery input, invalid PHP) deliberately switches the
//! walk state for everything that follows the enclosing block:
//! tolerated and deterministic, never a failure.

use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode};

/// One declaration node with its enclosing namespace (`""` is global)
/// and, for a direct member of a class-like, the item index of that
/// class-like.
pub(crate) struct ItemNode {
    pub(crate) node: SyntaxNode,
    pub(crate) namespace: String,
    pub(crate) owner: Option<u32>,
}

/// Every declaration node of the file, in tree order.
pub(crate) fn item_nodes(root: &SyntaxNode) -> Vec<ItemNode> {
    let mut items = Vec::new();
    let mut namespace = String::new();
    collect(root, &mut namespace, None, &mut items);
    items
}

fn push(
    items: &mut Vec<ItemNode>,
    node: &SyntaxNode,
    namespace: &str,
    owner: Option<u32>,
) -> Option<u32> {
    let index = u32::try_from(items.len()).ok()?;
    items.push(ItemNode {
        node: node.clone(),
        namespace: namespace.to_owned(),
        owner,
    });
    Some(index)
}

fn collect(
    node: &SyntaxNode,
    namespace: &mut String,
    owner: Option<u32>,
    items: &mut Vec<ItemNode>,
) {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::NamespaceDeclaration => {
                push(items, &child, namespace, None);
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
                        collect(block.syntax(), &mut inner, None, items);
                    }
                    // Statement form: the name applies to what follows.
                    None => *namespace = declared,
                }
            }
            _ => {
                if is_class_like(&child) {
                    // Named or anonymous: both are numbered; the
                    // anonymous one's position is its identity. Direct
                    // members walk under this index; everything deeper
                    // (method bodies) walks ownerless again.
                    let index = push(items, &child, namespace, owner);
                    collect(&child, namespace, index, items);
                    continue;
                }
                if is_member(&child, owner) || is_ownerless_item(&child) {
                    push(
                        items,
                        &child,
                        namespace,
                        if is_member(&child, owner) {
                            owner
                        } else {
                            None
                        },
                    );
                }
                // Members own only their list-level children: a method
                // body's declarations are ownerless.
                let descend_owner = if child.kind() == SyntaxKind::MemberList {
                    owner
                } else {
                    None
                };
                collect(&child, namespace, descend_owner, items);
            }
        }
    }
}

/// Any class-like declaration node, named or not: classes, interfaces,
/// traits, enums. Interfaces, traits, and enums are always named in
/// valid PHP, but error recovery may produce nameless ones; a nameless
/// class-like is numbered (anonymous classes need the identity) and
/// projected by nobody unless a projection wants it.
fn is_class_like(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::ClassDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::TraitDeclaration
            | SyntaxKind::EnumDeclaration
    )
}

/// A direct member of the enclosing class-like: numbered under its
/// owner. `owner` is set exactly while walking a member list.
fn is_member(node: &SyntaxNode, owner: Option<u32>) -> bool {
    owner.is_some()
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == SyntaxKind::MemberList)
        && matches!(
            node.kind(),
            SyntaxKind::MethodDeclaration
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::ConstantDeclaration
                | SyntaxKind::EnumCase
        )
}

/// The ownerless item kinds, exactly the old `is_item` minus the
/// class-likes (handled above). A `ConstantDeclaration` here is a
/// top-level `const`: the member form is caught by `is_member` first.
fn is_ownerless_item(node: &SyntaxNode) -> bool {
    match node.kind() {
        SyntaxKind::UseDeclaration | SyntaxKind::ConstantDeclaration => true,
        SyntaxKind::FunctionDeclaration => ast::FunctionDeclaration::cast(node.clone())
            .is_some_and(|declaration| declaration.name_token().is_some()),
        _ => false,
    }
}
