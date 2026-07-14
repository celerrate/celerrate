//! The member projection: the sibling of the item tree at member
//! granularity. Per class-like declaration of one file, its direct
//! members — kind, name, flags, identity, and (Task 4) signature as
//! unresolved names plus docblock text. Range-free and
//! `Eq`-comparable: a method body edit produces an identical value,
//! salsa backdates it, and member consumers are spared, while the
//! top-level `ItemTree` never changes at all — which is what spares
//! `source_symbol_table` (the settled serial-rebuild debt).

use celerrate_source::FileId;
use celerrate_syntax::ast::{self, AstNode};
use celerrate_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::ast_id::AstId;
use crate::item_nodes::{ItemNode, item_nodes};
use crate::items::DeclarationKind;

/// The kind of one class member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberKind {
    Method,
    Property,
    ClassConstant,
    EnumCase,
}

/// PHP member visibility. Unwritten and `var` are public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

/// The modifier flags of one member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MemberFlags {
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_readonly: bool,
}

impl Default for MemberFlags {
    fn default() -> Self {
        Self {
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: false,
            is_readonly: false,
        }
    }
}

/// One member: original spelling (property names without the `$`),
/// flags, and stable identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Member {
    pub kind: MemberKind,
    pub name: String,
    pub flags: MemberFlags,
    pub ast_id: AstId,
}

/// One class-like declaration and its direct members, in tree order.
/// `name` is `None` for anonymous class-likes; their `ast_id` is the
/// synthetic identity the spec gives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMembers {
    pub kind: DeclarationKind,
    pub name: Option<String>,
    pub namespace: String,
    pub ast_id: AstId,
    pub members: Vec<Member>,
}

/// The member projection of one file, in tree order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemberTree {
    pub classes: Vec<ClassMembers>,
}

impl MemberTree {
    /// Projects one file's syntax tree. Shares the item-node traversal
    /// with `AstIdMap` and `ItemTree`, so all three numberings agree
    /// by construction.
    pub fn from_root(file: FileId, root: &SyntaxNode) -> Self {
        let nodes = item_nodes(root);
        let mut classes: Vec<(u32, ClassMembers)> = Vec::new();
        for (position, item) in nodes.iter().enumerate() {
            let Ok(index) = u32::try_from(position) else {
                break;
            };
            let ast_id = AstId { file, index };
            if let Some(group) = class_group(item, ast_id) {
                classes.push((index, group));
                continue;
            }
            let Some(owner) = item.owner else { continue };
            let Some((_, group)) = classes
                .iter_mut()
                .rev()
                .find(|(class_index, _)| *class_index == owner)
            else {
                continue;
            };
            lower_member(&item.node, ast_id, group);
        }
        Self {
            classes: classes.into_iter().map(|(_, group)| group).collect(),
        }
    }
}

/// The group of a class-like item node; `None` for anything else.
fn class_group(item: &ItemNode, ast_id: AstId) -> Option<ClassMembers> {
    let (kind, name_token) = match item.node.kind() {
        SyntaxKind::ClassDeclaration => {
            let declaration = ast::ClassDeclaration::cast(item.node.clone())?;
            (DeclarationKind::Class, declaration.name_token())
        }
        SyntaxKind::InterfaceDeclaration => {
            let declaration = ast::InterfaceDeclaration::cast(item.node.clone())?;
            (DeclarationKind::Interface, declaration.name_token())
        }
        SyntaxKind::TraitDeclaration => {
            let declaration = ast::TraitDeclaration::cast(item.node.clone())?;
            (DeclarationKind::Trait, declaration.name_token())
        }
        SyntaxKind::EnumDeclaration => {
            let declaration = ast::EnumDeclaration::cast(item.node.clone())?;
            (DeclarationKind::Enum, declaration.name_token())
        }
        _ => return None,
    };
    Some(ClassMembers {
        kind,
        name: name_token.map(|token| token.text().to_owned()),
        namespace: item.namespace.clone(),
        ast_id,
        members: Vec::new(),
    })
}

fn lower_member(node: &SyntaxNode, ast_id: AstId, group: &mut ClassMembers) {
    match node.kind() {
        SyntaxKind::MethodDeclaration => {
            let Some(method) = ast::MethodDeclaration::cast(node.clone()) else {
                return;
            };
            let Some(name) = method.name_token() else {
                return;
            };
            group.members.push(Member {
                kind: MemberKind::Method,
                name: name.text().to_owned(),
                flags: flags_of(method.modifiers()),
                ast_id,
            });
        }
        SyntaxKind::PropertyDeclaration => {
            let Some(property) = ast::PropertyDeclaration::cast(node.clone()) else {
                return;
            };
            let flags = flags_of(property.modifiers());
            for element in property.property_elements() {
                let Some(name) = element.name_token() else {
                    continue;
                };
                group.members.push(Member {
                    kind: MemberKind::Property,
                    name: property_name(&name),
                    flags,
                    ast_id,
                });
            }
        }
        SyntaxKind::ConstantDeclaration => {
            let Some(constant) = ast::ConstantDeclaration::cast(node.clone()) else {
                return;
            };
            let flags = flags_of(constant.modifiers());
            for element in constant.constant_elements() {
                let Some(name) = element.name_token() else {
                    continue;
                };
                group.members.push(Member {
                    kind: MemberKind::ClassConstant,
                    name: name.text().to_owned(),
                    flags,
                    ast_id,
                });
            }
        }
        SyntaxKind::EnumCase => {
            let Some(case) = ast::EnumCase::cast(node.clone()) else {
                return;
            };
            let Some(name) = case.name_token() else {
                return;
            };
            group.members.push(Member {
                kind: MemberKind::EnumCase,
                name: name.text().to_owned(),
                flags: MemberFlags::default(),
                ast_id,
            });
        }
        _ => {}
    }
}

/// The bare property name: the `$` sigil stripped from the variable
/// token. Lookup and reflection both use the bare name.
fn property_name(token: &SyntaxToken) -> String {
    token.text().trim_start_matches('$').to_owned()
}

fn flags_of(modifiers: impl Iterator<Item = SyntaxToken>) -> MemberFlags {
    let mut flags = MemberFlags::default();
    for token in modifiers {
        match token.kind() {
            SyntaxKind::Public | SyntaxKind::Var => flags.visibility = Visibility::Public,
            SyntaxKind::Protected => flags.visibility = Visibility::Protected,
            SyntaxKind::Private => flags.visibility = Visibility::Private,
            SyntaxKind::Static => flags.is_static = true,
            SyntaxKind::Abstract => flags.is_abstract = true,
            SyntaxKind::Final => flags.is_final = true,
            SyntaxKind::Readonly => flags.is_readonly = true,
            _ => {}
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_source::FileId;

    use super::{MemberKind, MemberTree, Visibility};
    use crate::items::DeclarationKind;

    fn tree_of(source: &str) -> MemberTree {
        MemberTree::from_root(FileId::new(0), &celerrate_syntax::parse(source).tree())
    }

    fn only_class(source: &str) -> super::ClassMembers {
        let tree = tree_of(source);
        assert_eq!(tree.classes.len(), 1, "expected one class-like");
        tree.classes.into_iter().next().unwrap()
    }

    #[test]
    fn every_member_kind_is_projected_in_tree_order() {
        let class = only_class(
            "<?php class A {\n\
                 const LIMIT = 1;\n\
                 public int $count = 0;\n\
                 public function method(): void {}\n\
             }",
        );
        let kinds_and_names: Vec<(MemberKind, &str)> = class
            .members
            .iter()
            .map(|member| (member.kind, member.name.as_str()))
            .collect();
        assert_eq!(
            kinds_and_names,
            vec![
                (MemberKind::ClassConstant, "LIMIT"),
                (MemberKind::Property, "count"),
                (MemberKind::Method, "method"),
            ],
        );
        assert_eq!(class.kind, DeclarationKind::Class);
        assert_eq!(class.name.as_deref(), Some("A"));
    }

    #[test]
    fn enum_cases_are_members_of_their_enum() {
        let class = only_class("<?php enum Suit: string { case Hearts = 'h'; case Spades = 's'; }");
        assert_eq!(class.kind, DeclarationKind::Enum);
        let names: Vec<&str> = class.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["Hearts", "Spades"]);
        assert!(class.members.iter().all(|m| m.kind == MemberKind::EnumCase));
    }

    #[test]
    fn flags_are_read_from_the_modifier_list() {
        let class = only_class(
            "<?php abstract class A {\n\
                 protected static readonly int $x;\n\
                 abstract public function f(): void;\n\
                 final private function g() {}\n\
                 var $legacy;\n\
             }",
        );
        let property = &class.members[0];
        assert_eq!(property.flags.visibility, Visibility::Protected);
        assert!(property.flags.is_static);
        assert!(property.flags.is_readonly);
        let abstract_method = &class.members[1];
        assert_eq!(abstract_method.flags.visibility, Visibility::Public);
        assert!(abstract_method.flags.is_abstract);
        let final_method = &class.members[2];
        assert_eq!(final_method.flags.visibility, Visibility::Private);
        assert!(final_method.flags.is_final);
        let legacy = &class.members[3];
        assert_eq!(legacy.flags.visibility, Visibility::Public);
    }

    #[test]
    fn a_grouped_property_projects_one_member_per_element_sharing_identity() {
        let class = only_class("<?php class A { public $first, $second; }");
        let names: Vec<&str> = class.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second"]);
        assert_eq!(class.members[0].ast_id, class.members[1].ast_id);
    }

    #[test]
    fn a_grouped_class_constant_projects_one_member_per_element() {
        let class = only_class("<?php class A { const B = 1, C = 2; }");
        let names: Vec<&str> = class.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["B", "C"]);
        assert!(
            class
                .members
                .iter()
                .all(|m| m.kind == MemberKind::ClassConstant)
        );
    }

    #[test]
    fn an_anonymous_class_is_a_nameless_group_with_identity() {
        let tree =
            tree_of("<?php function wrapper() { return new class { public function f() {} }; }");
        assert_eq!(tree.classes.len(), 1);
        let class = tree.classes.first().unwrap();
        assert_eq!(class.name, None);
        // Numbering: wrapper = 0, anonymous class = 1, method = 2.
        assert_eq!(class.ast_id.index, 1);
        assert_eq!(class.members.first().map(|m| m.name.as_str()), Some("f"));
    }

    #[test]
    fn a_nested_class_owns_its_own_members() {
        // An anonymous class inside a method body belongs to itself,
        // not to the enclosing class.
        let tree = tree_of(
            "<?php class Outer { function f() { return new class { function inner() {} }; } }",
        );
        assert_eq!(tree.classes.len(), 2);
        let outer = &tree.classes[0];
        let inner = &tree.classes[1];
        assert_eq!(outer.name.as_deref(), Some("Outer"));
        assert_eq!(
            outer
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            vec!["f"],
        );
        assert_eq!(inner.name, None);
        assert_eq!(
            inner
                .members
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            vec!["inner"],
        );
    }

    #[test]
    fn a_method_body_edit_produces_an_identical_member_tree() {
        let before = tree_of("<?php class A { function f() { return 1; } }");
        let body_edit = tree_of("<?php class A { function f() { return 2; } }");
        assert_eq!(before, body_edit);
    }

    #[test]
    fn interfaces_and_traits_group_their_members() {
        let tree = tree_of(
            "<?php interface I { public function f(); const K = 1; }\n\
             trait T { public function helper() {} }",
        );
        assert_eq!(tree.classes.len(), 2);
        assert_eq!(tree.classes[0].kind, DeclarationKind::Interface);
        assert_eq!(tree.classes[1].kind, DeclarationKind::Trait);
    }

    #[test]
    fn malformed_input_projects_what_the_parser_recovered() {
        let tree = tree_of("<?php class Broken { public function ok() {}");
        assert_eq!(tree.classes.len(), 1);
        assert_eq!(
            tree.classes[0].members.first().map(|m| m.name.as_str()),
            Some("ok"),
        );
    }

    #[test]
    fn empty_and_memberless_files_project_nothing_surprising() {
        assert_eq!(tree_of("").classes, Vec::new());
        assert_eq!(tree_of("<?php function free() {}").classes, Vec::new());
        let class = only_class("<?php class Empty {}");
        assert_eq!(class.members, Vec::new());
    }
}
