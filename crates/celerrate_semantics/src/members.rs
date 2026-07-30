//! The member projection: the sibling of the item tree at member
//! granularity. Per class-like declaration of one file, its direct
//! members — kind, name, flags, identity, and signature as
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

/// One parameter of a method signature, as written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParameterSignature {
    pub name: String,
    pub type_text: Option<String>,
    pub default_text: Option<String>,
    pub by_reference: bool,
    pub variadic: bool,
    pub is_promoted: bool,
}

/// One member's signature, every type an unresolved written text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct MemberSignature {
    pub parameters: Vec<ParameterSignature>,
    pub type_text: Option<String>,
    pub default_text: Option<String>,
    pub by_reference: bool,
}

/// One member: original spelling (property names without the `$`),
/// flags, signature, docblock text, and stable identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Member {
    pub kind: MemberKind,
    pub name: String,
    pub flags: MemberFlags,
    pub signature: MemberSignature,
    pub docblock: Option<String>,
    pub ast_id: AstId,
}

/// One `insteadof` or `as` adaptation, as written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TraitAdaptation {
    Precedence {
        trait_name: Option<String>,
        member: String,
        excluded: Vec<String>,
    },
    Alias {
        trait_name: Option<String>,
        member: String,
        visibility: Option<Visibility>,
        alias: Option<String>,
    },
}

/// One `use Trait, …;` clause of a class body, adaptations included.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TraitUse {
    pub names: Vec<String>,
    pub adaptations: Vec<TraitAdaptation>,
}

/// One free function of the file: signature-granular, exactly like a
/// class member, so a body edit backdates and a signature edit
/// invalidates precisely the signature's dependents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeFunction {
    pub name: String,
    pub namespace: String,
    pub signature: MemberSignature,
    pub docblock: Option<String>,
    pub ast_id: AstId,
}

/// One class-like declaration and its direct members, in tree order.
/// `name` is `None` for anonymous class-likes; their `ast_id` is
/// their synthetic identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassMembers {
    pub kind: DeclarationKind,
    pub name: Option<String>,
    pub namespace: String,
    pub ast_id: AstId,
    pub docblock: Option<String>,
    pub members: Vec<Member>,
    pub trait_uses: Vec<TraitUse>,
    /// The class-like attribute names as written (last segment kept by
    /// consumers), e.g. `AllowDynamicProperties`. Read from the
    /// declaration node's attribute groups so linearization need not
    /// re-read the syntax tree.
    pub attribute_names: Vec<String>,
    /// Written `extends` names, for the declaration-less anonymous
    /// case; named classes keep resolving heritage through their
    /// `Declaration`. Populated for every class-like, same accessors
    /// `ItemTree`'s lowering reads.
    pub extends: Vec<String>,
    /// Written `implements` names, same purpose.
    pub implements: Vec<String>,
}

/// The member projection of one file, in tree order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemberTree {
    pub classes: Vec<ClassMembers>,
    pub functions: Vec<FreeFunction>,
}

impl MemberTree {
    /// Projects one file's syntax tree. Shares the item-node traversal
    /// with `AstIdMap` and `ItemTree`, so all three numberings agree
    /// by construction.
    pub fn from_root(file: FileId, root: &SyntaxNode) -> Self {
        let nodes = item_nodes(root);
        let mut classes: Vec<(u32, ClassMembers)> = Vec::new();
        let mut functions = Vec::new();
        for (position, item) in nodes.iter().enumerate() {
            let Ok(index) = u32::try_from(position) else {
                break;
            };
            let ast_id = AstId { file, index };
            // Checked before the owner check below: a class-like sitting
            // directly inside a member list (invalid PHP, error recovery)
            // still gets its own member group here, even though `owner`
            // is `Some` and `ItemTree::from_root` skips it as a member.
            // Intentional asymmetry: the top-level projection ignores a
            // recovered nested class-like, but its members still need a
            // home.
            if let Some(group) = class_group(item, ast_id) {
                classes.push((index, group));
                continue;
            }
            if item.owner.is_none() && item.node.kind() == SyntaxKind::FunctionDeclaration {
                if let Some(function) = free_function(item, ast_id) {
                    functions.push(function);
                }
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
            functions,
        }
    }
}

/// The free function of a top-level `FunctionDeclaration` item node;
/// `None` when the node fails to cast or carries no name (defensive
/// only: the item walk admits named function declarations exclusively).
fn free_function(item: &ItemNode, ast_id: AstId) -> Option<FreeFunction> {
    let function = ast::FunctionDeclaration::cast(item.node.clone())?;
    let name = function.name_token()?;
    Some(FreeFunction {
        name: name.text().to_owned(),
        namespace: item.namespace.clone(),
        signature: MemberSignature {
            parameters: parameter_signatures(function.parameter_list()),
            type_text: function.return_type().map(|ty| ast::type_text(&ty)),
            default_text: None,
            by_reference: function.by_reference_token().is_some(),
        },
        docblock: ast::docblock_token(&item.node).map(|token| token.text().to_owned()),
        ast_id,
    })
}

/// The group of a class-like item node; `None` for anything else.
fn class_group(item: &ItemNode, ast_id: AstId) -> Option<ClassMembers> {
    let (kind, name_token, trait_uses, attribute_names, extends, implements) = match item
        .node
        .kind()
    {
        SyntaxKind::ClassDeclaration => {
            let declaration = ast::ClassDeclaration::cast(item.node.clone())?;
            let trait_uses = trait_uses_of(declaration.member_list());
            let attribute_names = attribute_names_of(declaration.attribute_groups());
            let extends = names_of(declaration.extends_clause().map(|clause| clause.names()));
            let implements = names_of(declaration.implements_clause().map(|clause| clause.names()));
            (
                DeclarationKind::Class,
                declaration.name_token(),
                trait_uses,
                attribute_names,
                extends,
                implements,
            )
        }
        SyntaxKind::InterfaceDeclaration => {
            let declaration = ast::InterfaceDeclaration::cast(item.node.clone())?;
            let trait_uses = trait_uses_of(declaration.member_list());
            let attribute_names = attribute_names_of(declaration.attribute_groups());
            let extends = names_of(declaration.extends_clause().map(|clause| clause.names()));
            let implements = names_of(declaration.implements_clause().map(|clause| clause.names()));
            (
                DeclarationKind::Interface,
                declaration.name_token(),
                trait_uses,
                attribute_names,
                extends,
                implements,
            )
        }
        SyntaxKind::TraitDeclaration => {
            let declaration = ast::TraitDeclaration::cast(item.node.clone())?;
            let trait_uses = trait_uses_of(declaration.member_list());
            let attribute_names = attribute_names_of(declaration.attribute_groups());
            let extends = names_of(declaration.extends_clause().map(|clause| clause.names()));
            let implements = names_of(declaration.implements_clause().map(|clause| clause.names()));
            (
                DeclarationKind::Trait,
                declaration.name_token(),
                trait_uses,
                attribute_names,
                extends,
                implements,
            )
        }
        SyntaxKind::EnumDeclaration => {
            let declaration = ast::EnumDeclaration::cast(item.node.clone())?;
            let trait_uses = trait_uses_of(declaration.member_list());
            let attribute_names = attribute_names_of(declaration.attribute_groups());
            let extends = names_of(declaration.extends_clause().map(|clause| clause.names()));
            let implements = names_of(declaration.implements_clause().map(|clause| clause.names()));
            (
                DeclarationKind::Enum,
                declaration.name_token(),
                trait_uses,
                attribute_names,
                extends,
                implements,
            )
        }
        _ => return None,
    };
    Some(ClassMembers {
        kind,
        name: name_token.map(|token| token.text().to_owned()),
        namespace: item.namespace.clone(),
        ast_id,
        docblock: ast::docblock_token(&item.node).map(|token| token.text().to_owned()),
        members: Vec::new(),
        trait_uses,
        attribute_names,
        extends,
        implements,
    })
}

/// The written names of one heritage clause, in source order. Mirrors
/// `items.rs`'s `ItemTree` projection of the same clauses — the member
/// projection and the item projection are siblings with no dependency
/// edge between them, so the reader is duplicated rather than shared.
fn names_of(names: Option<ast::AstChildren<ast::Name>>) -> Vec<String> {
    names
        .into_iter()
        .flatten()
        .map(|name| name.text())
        .collect()
}

/// The written names of every attribute across a class-like's attribute
/// groups, in tree order, e.g. `[AllowDynamicProperties]`.
fn attribute_names_of(groups: ast::AstChildren<ast::AttributeGroup>) -> Vec<String> {
    groups
        .flat_map(|group| group.attributes())
        .filter_map(|attribute| attribute.name().map(|name| name.text()))
        .collect()
}

/// The trait-use clauses of a class-like body, adaptations resolved.
/// Trait-use clauses are not numbered items, so this reads the class
/// node's member list directly at group creation.
fn trait_uses_of(member_list: Option<ast::MemberList>) -> Vec<TraitUse> {
    member_list
        .into_iter()
        .flat_map(|list| list.member_declarations())
        .filter_map(|member| match member {
            ast::MemberDeclaration::TraitUseClause(clause) => Some(clause),
            _ => None,
        })
        .map(|clause| TraitUse {
            names: clause.names().map(|name| name.text()).collect(),
            adaptations: clause
                .trait_adaptation_list()
                .into_iter()
                .flat_map(|list| list.trait_adaptations())
                .filter_map(|adaptation| lower_adaptation(&adaptation))
                .collect(),
        })
        .collect()
}

/// Whether the adaptation node contains a `::` separator: the
/// discriminator between a qualified reference (`A::m`) and the
/// unqualified form, where `reference_name` returns the member itself.
fn has_colon_colon(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .any(|token| token.kind() == SyntaxKind::ColonColon)
}

fn lower_adaptation(adaptation: &ast::TraitAdaptation) -> Option<TraitAdaptation> {
    match adaptation {
        ast::TraitAdaptation::TraitPrecedence(precedence) => {
            let member = precedence.member_token()?.text().to_owned();
            let trait_name = precedence
                .reference_name()
                .map(|name| name.text())
                .filter(|_| has_colon_colon(precedence.syntax()));
            let excluded = precedence
                .excluded_names()
                .map(|name| name.text())
                .collect();
            Some(TraitAdaptation::Precedence {
                trait_name,
                member,
                excluded,
            })
        }
        ast::TraitAdaptation::TraitAlias(alias) => {
            let member_token = alias.member_token()?;
            let member = member_token.text().to_owned();
            let trait_name = alias
                .reference_name()
                .map(|name| name.text())
                .filter(|_| has_colon_colon(alias.syntax()));
            let visibility = alias.visibility_token().map(|token| match token.kind() {
                SyntaxKind::Protected => Visibility::Protected,
                SyntaxKind::Private => Visibility::Private,
                _ => Visibility::Public,
            });
            let alias_name = alias
                .alias_token()
                .filter(|token| token.text_range() != member_token.text_range())
                .map(|token| token.text().to_owned());
            Some(TraitAdaptation::Alias {
                trait_name,
                member,
                visibility,
                alias: alias_name,
            })
        }
    }
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
                signature: method_signature(&method),
                docblock: ast::docblock_token(node).map(|token| token.text().to_owned()),
                ast_id,
            });
            // Promotion is a constructor-only PHP feature: guard on the
            // method name so an unrelated method's modifier-bearing
            // parameter (invalid PHP, error recovery) never surfaces as
            // a property.
            if name.text().eq_ignore_ascii_case("__construct") {
                for parameter in method
                    .parameter_list()
                    .into_iter()
                    .flat_map(|list| list.parameters())
                {
                    if !parameter_is_promoted(&parameter) {
                        continue;
                    }
                    let mut flags = flags_of(parameter.modifiers());
                    flags.is_static = false;
                    let Some(parameter_name) = parameter.name_token() else {
                        continue;
                    };
                    group.members.push(Member {
                        kind: MemberKind::Property,
                        name: parameter_name.text().trim_start_matches('$').to_owned(),
                        flags,
                        signature: MemberSignature {
                            type_text: parameter.ty().map(|ty| ast::type_text(&ty)),
                            default_text: parameter
                                .default_value()
                                .map(|expression| ast::expression_text(&expression)),
                            ..MemberSignature::default()
                        },
                        // A promoted parameter can carry its own `@var`
                        // docblock immediately ahead of it in the
                        // parameter list (the untyped-array-element
                        // idiom native syntax cannot express); it is a
                        // preceding sibling of the parameter node
                        // itself, exactly what `docblock_token` finds.
                        docblock: ast::docblock_token(parameter.syntax())
                            .map(|token| token.text().to_owned()),
                        ast_id,
                    });
                }
            }
        }
        SyntaxKind::PropertyDeclaration => {
            let Some(property) = ast::PropertyDeclaration::cast(node.clone()) else {
                return;
            };
            let flags = flags_of(property.modifiers());
            let type_text = property.ty().map(|ty| ast::type_text(&ty));
            let docblock = ast::docblock_token(node).map(|token| token.text().to_owned());
            for element in property.property_elements() {
                let Some(name) = element.name_token() else {
                    continue;
                };
                group.members.push(Member {
                    kind: MemberKind::Property,
                    name: property_name(&name),
                    flags,
                    signature: MemberSignature {
                        type_text: type_text.clone(),
                        default_text: element.expression().map(|e| ast::expression_text(&e)),
                        ..MemberSignature::default()
                    },
                    docblock: docblock.clone(),
                    ast_id,
                });
            }
        }
        SyntaxKind::ConstantDeclaration => {
            let Some(constant) = ast::ConstantDeclaration::cast(node.clone()) else {
                return;
            };
            let flags = flags_of(constant.modifiers());
            let type_text = constant.ty().map(|ty| ast::type_text(&ty));
            let docblock = ast::docblock_token(node).map(|token| token.text().to_owned());
            for element in constant.constant_elements() {
                let Some(name) = element.name_token() else {
                    continue;
                };
                group.members.push(Member {
                    kind: MemberKind::ClassConstant,
                    name: name.text().to_owned(),
                    flags,
                    signature: MemberSignature {
                        type_text: type_text.clone(),
                        default_text: element.value().map(|e| ast::expression_text(&e)),
                        ..MemberSignature::default()
                    },
                    docblock: docblock.clone(),
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
                signature: MemberSignature {
                    default_text: case.value().map(|e| ast::expression_text(&e)),
                    ..MemberSignature::default()
                },
                docblock: ast::docblock_token(node).map(|token| token.text().to_owned()),
                ast_id,
            });
        }
        _ => {}
    }
}

/// The written signatures of a parameter list: shared between method
/// signatures and the body IR's closures, so both stay byte-compatible.
pub(crate) fn parameter_signatures(list: Option<ast::ParameterList>) -> Vec<ParameterSignature> {
    list.into_iter()
        .flat_map(|list| list.parameters())
        .filter_map(|parameter| {
            let name = parameter.name_token()?;
            Some(ParameterSignature {
                name: name.text().trim_start_matches('$').to_owned(),
                type_text: parameter.ty().map(|ty| ast::type_text(&ty)),
                default_text: parameter
                    .default_value()
                    .map(|expression| ast::expression_text(&expression)),
                by_reference: parameter.by_reference_token().is_some(),
                variadic: parameter.variadic_token().is_some(),
                is_promoted: parameter.modifiers().next().is_some(),
            })
        })
        .collect()
}

/// Whether a parameter carries constructor-promotion modifiers, e.g.
/// `private readonly`. Mirrors the `is_promoted` rule of
/// `parameter_signatures`.
fn parameter_is_promoted(parameter: &ast::Parameter) -> bool {
    parameter.modifiers().next().is_some()
}

/// One method's signature, as written: every type an unresolved text,
/// no type resolution.
fn method_signature(method: &ast::MethodDeclaration) -> MemberSignature {
    MemberSignature {
        parameters: parameter_signatures(method.parameter_list()),
        type_text: method.return_type().map(|ty| ast::type_text(&ty)),
        default_text: None,
        by_reference: method.by_reference_token().is_some(),
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

    use super::{Member, MemberKind, MemberTree, Visibility};
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

    #[test]
    fn a_method_signature_carries_parameters_return_type_and_reference() {
        let class = only_class(
            "<?php class A {\n\
                 public function f(int $count, Foo\\Bar|null $subject = null, string ...$rest): static {}\n\
                 public function &g() {}\n\
             }",
        );
        let f = &class.members[0];
        let parameters = &f.signature.parameters;
        assert_eq!(parameters.len(), 3);
        assert_eq!(parameters[0].name, "count");
        assert_eq!(parameters[0].type_text.as_deref(), Some("int"));
        assert_eq!(parameters[0].default_text, None);
        assert_eq!(parameters[1].type_text.as_deref(), Some("Foo\\Bar|null"));
        assert_eq!(parameters[1].default_text.as_deref(), Some("null"));
        assert!(parameters[2].variadic);
        assert_eq!(f.signature.type_text.as_deref(), Some("static"));
        assert!(!f.signature.by_reference);
        assert!(class.members[1].signature.by_reference);
    }

    #[test]
    fn promoted_constructor_parameters_are_marked() {
        let class = only_class(
            "<?php class A { public function __construct(private readonly int $id, string $plain) {} }",
        );
        let constructor = &class.members[0];
        assert!(constructor.signature.parameters[0].is_promoted);
        assert!(!constructor.signature.parameters[1].is_promoted);
    }

    #[test]
    fn property_and_constant_signatures_carry_type_and_default() {
        let class = only_class(
            "<?php class A {\n\
                 public ?Logger $logger = null;\n\
                 public array $bare = [];\n\
                 final const int LIMIT = 10;\n\
             }",
        );
        let logger = &class.members[0];
        assert_eq!(logger.signature.type_text.as_deref(), Some("?Logger"));
        assert_eq!(logger.signature.default_text.as_deref(), Some("null"));
        let limit = &class.members[2];
        assert_eq!(limit.signature.type_text.as_deref(), Some("int"));
        assert_eq!(limit.signature.default_text.as_deref(), Some("10"));
    }

    #[test]
    fn an_enum_case_value_is_its_default_text() {
        let class = only_class("<?php enum Suit: string { case Hearts = 'h'; case Clubs; }");
        assert_eq!(
            class.members[0].signature.default_text.as_deref(),
            Some("'h'"),
        );
        assert_eq!(class.members[1].signature.default_text, None);
    }

    #[test]
    fn docblocks_attach_to_members_and_to_the_class() {
        let class = only_class(
            "<?php\n\
             /** @template T */\n\
             class Collection {\n\
                 /** @return T|null */\n\
                 public function first() {}\n\
                 public function undocumented() {}\n\
             }",
        );
        assert_eq!(class.docblock.as_deref(), Some("/** @template T */"));
        assert_eq!(
            class.members[0].docblock.as_deref(),
            Some("/** @return T|null */"),
        );
        assert_eq!(class.members[1].docblock, None);
    }

    #[test]
    fn a_docblock_edit_changes_the_member_tree_a_body_comment_does_not() {
        // An accepted cost, pinned: docblock text is a field,
        // so editing it changes the value; a comment inside a body is
        // still invisible.
        let before = tree_of("<?php class A { /** @return int */ function f() { return 1; } }");
        let docblock_edit =
            tree_of("<?php class A { /** @return string */ function f() { return 1; } }");
        let body_comment_edit =
            tree_of("<?php class A { /** @return int */ function f() { /* note */ return 1; } }");
        assert_ne!(before, docblock_edit);
        assert_eq!(before, body_comment_edit);
    }

    #[test]
    fn a_default_value_edit_changes_the_member_tree_formatting_does_not() {
        // The comparable form is the projection typed judgments read:
        // content changes invalidate, formatting does not.
        let before = tree_of("<?php class A { public $x = new Foo(1, 2); }");
        let content_edit = tree_of("<?php class A { public $x = new Foo(1, 3); }");
        let formatting_edit = tree_of("<?php class A { public $x = new  Foo( 1,   2 ); }");
        assert_ne!(before, content_edit);
        assert_eq!(before, formatting_edit);
    }

    use super::{TraitAdaptation, TraitUse};

    #[test]
    fn trait_uses_carry_their_names() {
        let class = only_class("<?php class A { use First, Concerns\\Second; use \\Third; }");
        assert_eq!(
            class.trait_uses,
            vec![
                TraitUse {
                    names: vec!["First".to_owned(), "Concerns\\Second".to_owned()],
                    adaptations: Vec::new(),
                },
                TraitUse {
                    names: vec!["\\Third".to_owned()],
                    adaptations: Vec::new(),
                },
            ],
        );
    }

    #[test]
    fn insteadof_and_as_adaptations_are_captured() {
        let class = only_class(
            "<?php class A {\n\
                 use B, C {\n\
                     B::hello insteadof C;\n\
                     C::hello as protected hi;\n\
                     bye as farewell;\n\
                 }\n\
             }",
        );
        let adaptations = &class.trait_uses.first().unwrap().adaptations;
        assert_eq!(
            adaptations[0],
            TraitAdaptation::Precedence {
                trait_name: Some("B".to_owned()),
                member: "hello".to_owned(),
                excluded: vec!["C".to_owned()],
            },
        );
        assert_eq!(
            adaptations[1],
            TraitAdaptation::Alias {
                trait_name: Some("C".to_owned()),
                member: "hello".to_owned(),
                visibility: Some(Visibility::Protected),
                alias: Some("hi".to_owned()),
            },
        );
        assert_eq!(
            adaptations[2],
            TraitAdaptation::Alias {
                trait_name: None,
                member: "bye".to_owned(),
                visibility: None,
                alias: Some("farewell".to_owned()),
            },
        );
    }

    #[test]
    fn a_visibility_only_alias_has_no_new_name() {
        let class = only_class("<?php class A { use B { hello as protected; } }");
        assert_eq!(
            class.trait_uses.first().unwrap().adaptations.first(),
            Some(&TraitAdaptation::Alias {
                trait_name: None,
                member: "hello".to_owned(),
                visibility: Some(Visibility::Protected),
                alias: None,
            }),
        );
    }

    #[test]
    fn free_functions_project_their_signatures() {
        let tree = tree_of(
            "<?php namespace App;\n\
             /** doc */\n\
             function build(int $count, string ...$names): ?Widget { return null; }",
        );
        assert_eq!(tree.functions.len(), 1);
        let function = &tree.functions[0];
        assert_eq!(function.name, "build");
        assert_eq!(function.namespace, "App");
        assert_eq!(function.docblock.as_deref(), Some("/** doc */"));
        assert_eq!(function.signature.type_text.as_deref(), Some("?Widget"));
        assert_eq!(function.signature.parameters.len(), 2);
        assert_eq!(function.signature.parameters[0].name, "count");
        assert_eq!(
            function.signature.parameters[0].type_text.as_deref(),
            Some("int"),
        );
        assert!(function.signature.parameters[1].variadic);
    }

    #[test]
    fn a_function_body_edit_leaves_the_member_tree_identical() {
        let before = tree_of("<?php function f(int $x): int { return $x; }");
        let after = tree_of("<?php function f(int $x): int { return $x + 1; }");
        assert_eq!(before, after);
    }

    #[test]
    fn promoted_constructor_parameters_surface_as_properties() {
        let tree = tree_of(
            "<?php class Service {\n\
                 public function __construct(\n\
                     private readonly ?Logger $logger = null,\n\
                     int $plain = 0,\n\
                 ) {}\n\
             }",
        );
        let class = &tree.classes[0];
        let properties: Vec<&Member> = class
            .members
            .iter()
            .filter(|member| member.kind == MemberKind::Property)
            .collect();
        assert_eq!(properties.len(), 1, "only the promoted parameter");
        let promoted = properties[0];
        assert_eq!(promoted.name, "logger");
        assert_eq!(promoted.signature.type_text.as_deref(), Some("?Logger"));
        assert_eq!(promoted.signature.default_text.as_deref(), Some("null"));
        assert_eq!(promoted.flags.visibility, Visibility::Private);
        assert!(promoted.flags.is_readonly);
    }

    #[test]
    fn a_promoted_parameter_s_own_docblock_is_captured() {
        // A `@var` docblock directly ahead of a promoted parameter is
        // valid, common PHPDoc (symfony/demo's own AppExtension writes
        // exactly this to type an untyped-by-native-syntax array
        // property). Ground-truth harness triage found this
        // dropped silently: the promoted property carried no docblock
        // at all, so its element type never reached inference.
        let tree = tree_of(
            "<?php class Service {\n\
                 public function __construct(\n\
                     /** @var string[] */\n\
                     private readonly array $names,\n\
                 ) {}\n\
             }",
        );
        let class = &tree.classes[0];
        let promoted = class
            .members
            .iter()
            .find(|member| member.kind == MemberKind::Property)
            .unwrap();
        assert_eq!(promoted.docblock.as_deref(), Some("/** @var string[] */"));
    }
}
