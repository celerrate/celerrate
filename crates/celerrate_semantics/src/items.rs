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

/// The written names of one clause, in source order.
fn names_of(names: Option<ast::AstChildren<ast::Name>>) -> Vec<String> {
    names
        .into_iter()
        .flatten()
        .map(|name| name.text())
        .collect()
}

/// The trait names a class-like uses, read from its member list. The
/// traversal never descends into member lists; this projection field
/// is the one place the tree looks inside one, because the spec names
/// trait `use` among the inheritance names.
fn trait_use_names(member_list: Option<ast::MemberList>) -> Vec<String> {
    member_list
        .into_iter()
        .flat_map(|list| list.member_declarations())
        .filter_map(|member| match member {
            ast::MemberDeclaration::TraitUseClause(clause) => Some(clause),
            _ => None,
        })
        .flat_map(|clause| clause.names())
        .map(|name| name.text())
        .collect()
}

/// The unresolved inheritance names of one class-like declaration. The
/// four generated class-like types share accessor names but no trait;
/// this macro reads them uniformly.
macro_rules! inheritance_of {
    ($declaration:expr) => {
        Inheritance {
            extends: names_of($declaration.extends_clause().map(|clause| clause.names())),
            implements: names_of(
                $declaration
                    .implements_clause()
                    .map(|clause| clause.names()),
            ),
            trait_uses: trait_use_names($declaration.member_list()),
        }
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
                    inheritance_of!(declaration),
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
                    inheritance_of!(declaration),
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
                    inheritance_of!(declaration),
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
                    inheritance_of!(declaration),
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
        SyntaxKind::UseDeclaration => {
            if let Some(declaration) = ast::UseDeclaration::cast(item.node.clone()) {
                let inherited =
                    import_kind_of(declaration.import_type_token()).unwrap_or(ImportKind::Class);
                for clause in declaration.use_clauses() {
                    expand_use_clause(&clause, inherited, "", item, ast_id, tree);
                }
            }
        }
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

/// The import kind named by a `function` / `const` token, when present.
fn import_kind_of(token: Option<SyntaxToken>) -> Option<ImportKind> {
    match token?.kind() {
        SyntaxKind::Function => Some(ImportKind::Function),
        SyntaxKind::Const => Some(ImportKind::Constant),
        _ => None,
    }
}

/// Expands one `use` clause: a plain clause becomes one import, a
/// group form recurses with the accumulated prefix. Wreckage without a
/// usable target expands to nothing.
fn expand_use_clause(
    clause: &ast::UseClause,
    inherited: ImportKind,
    prefix: &str,
    item: &ItemNode,
    ast_id: AstId,
    tree: &mut ItemTree,
) {
    let kind = import_kind_of(clause.import_type_token()).unwrap_or(inherited);
    let written = clause.name().map(|name| name.text()).unwrap_or_default();
    let target = join_qualified(prefix, written.trim_start_matches('\\'));
    if let Some(group) = clause.use_group() {
        for inner in group.use_clauses() {
            expand_use_clause(&inner, kind, &target, item, ast_id, tree);
        }
        return;
    }
    if target.is_empty() {
        return;
    }
    let alias = clause
        .alias_token()
        .map(|token| token.text().to_owned())
        .unwrap_or_else(|| last_segment(&target).to_owned());
    tree.imports.push(UseImport {
        kind,
        target,
        alias,
        namespace: item.namespace.clone(),
        ast_id,
    });
}

fn join_qualified(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else if name.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}\\{name}")
    }
}

fn last_segment(target: &str) -> &str {
    target.rsplit('\\').next().unwrap_or(target)
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

    fn only_declaration(source: &str) -> super::Declaration {
        let tree = tree_of(source);
        assert_eq!(tree.declarations.len(), 1, "expected one declaration");
        tree.declarations.into_iter().next().unwrap()
    }

    #[test]
    fn a_class_carries_its_unresolved_inheritance_names() {
        let class = only_declaration(
            "<?php namespace App;\n\
             class Service extends \\Core\\Base implements Contract, \\Psr\\Log\\LoggerAwareInterface {\n\
                 use Concerns\\Loggable;\n\
                 use \\Shared\\Serializable;\n\
             }\n",
        );
        assert_eq!(class.extends, vec!["\\Core\\Base".to_owned()]);
        assert_eq!(
            class.implements,
            vec![
                "Contract".to_owned(),
                "\\Psr\\Log\\LoggerAwareInterface".to_owned(),
            ],
        );
        assert_eq!(
            class.trait_uses,
            vec![
                "Concerns\\Loggable".to_owned(),
                "\\Shared\\Serializable".to_owned(),
            ],
        );
    }

    #[test]
    fn an_interface_extends_many_parents() {
        let interface = only_declaration("<?php interface Both extends First, Second\\Third {}");
        assert_eq!(
            interface.extends,
            vec!["First".to_owned(), "Second\\Third".to_owned()],
        );
        assert_eq!(interface.implements, Vec::<String>::new());
    }

    #[test]
    fn an_enum_carries_its_implements_names() {
        let declaration =
            only_declaration("<?php enum Suit: string implements HasColor { use Colored; }");
        assert_eq!(declaration.implements, vec!["HasColor".to_owned()]);
        assert_eq!(declaration.trait_uses, vec!["Colored".to_owned()]);
    }

    #[test]
    fn a_grouped_trait_use_lists_every_name() {
        let class = only_declaration("<?php class Mixed { use A, B\\C; }");
        assert_eq!(class.trait_uses, vec!["A".to_owned(), "B\\C".to_owned()],);
    }

    #[test]
    fn functions_and_constants_carry_no_inheritance() {
        let function = only_declaration("<?php function greet() {}");
        assert_eq!(function.extends, Vec::<String>::new());
        assert_eq!(function.implements, Vec::<String>::new());
        assert_eq!(function.trait_uses, Vec::<String>::new());
    }

    use super::{ImportKind, UseImport};

    fn imports_of(source: &str) -> Vec<UseImport> {
        tree_of(source).imports
    }

    fn targets_and_aliases(source: &str) -> Vec<(ImportKind, String, String)> {
        imports_of(source)
            .into_iter()
            .map(|import| (import.kind, import.target, import.alias))
            .collect()
    }

    #[test]
    fn a_simple_use_imports_a_class_with_its_last_segment_as_alias() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Bar".to_owned())],
        );
    }

    #[test]
    fn a_leading_backslash_is_trimmed_from_the_target() {
        // Use targets are always absolute; the written backslash adds
        // nothing.
        assert_eq!(
            targets_and_aliases("<?php use \\Foo\\Bar;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Bar".to_owned())],
        );
    }

    #[test]
    fn an_explicit_alias_wins() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar as Baz;"),
            vec![(ImportKind::Class, "Foo\\Bar".to_owned(), "Baz".to_owned())],
        );
    }

    #[test]
    fn function_and_const_declarations_set_the_import_kind() {
        assert_eq!(
            targets_and_aliases("<?php use function Foo\\greet; use const Foo\\LIMIT;"),
            vec![
                (
                    ImportKind::Function,
                    "Foo\\greet".to_owned(),
                    "greet".to_owned()
                ),
                (
                    ImportKind::Constant,
                    "Foo\\LIMIT".to_owned(),
                    "LIMIT".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn a_group_expands_with_the_shared_prefix() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\Bar\\{Baz, Qux\\Deep as D};"),
            vec![
                (
                    ImportKind::Class,
                    "Foo\\Bar\\Baz".to_owned(),
                    "Baz".to_owned()
                ),
                (
                    ImportKind::Class,
                    "Foo\\Bar\\Qux\\Deep".to_owned(),
                    "D".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn a_mixed_group_overrides_the_kind_per_clause() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\{function greet, const LIMIT, Service};",),
            vec![
                (
                    ImportKind::Function,
                    "Foo\\greet".to_owned(),
                    "greet".to_owned()
                ),
                (
                    ImportKind::Constant,
                    "Foo\\LIMIT".to_owned(),
                    "LIMIT".to_owned()
                ),
                (
                    ImportKind::Class,
                    "Foo\\Service".to_owned(),
                    "Service".to_owned()
                ),
            ],
        );
    }

    #[test]
    fn comma_separated_clauses_each_import() {
        assert_eq!(
            targets_and_aliases("<?php use Foo\\A, Foo\\B;"),
            vec![
                (ImportKind::Class, "Foo\\A".to_owned(), "A".to_owned()),
                (ImportKind::Class, "Foo\\B".to_owned(), "B".to_owned()),
            ],
        );
    }

    #[test]
    fn imports_carry_their_enclosing_namespace_and_identity() {
        let tree = tree_of("<?php namespace App; use Lib\\Helper;");
        let import = tree.imports.first().unwrap();
        assert_eq!(import.namespace, "App");
        // Numbering: namespace = 0, use declaration = 1.
        assert_eq!(import.ast_id.index, 1);
    }
}
