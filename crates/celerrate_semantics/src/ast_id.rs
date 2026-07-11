use celerrate_source::FileId;
use celerrate_syntax::{SyntaxNode, SyntaxNodePtr};

use crate::item_nodes::item_nodes;

/// The stable identity of one declaration: the file plus the
/// declaration's position in the file's tree-order numbering. A body
/// edit renumbers nothing, so an `AstId` survives everyday editing;
/// reconciliation back to the concrete node goes through [`AstIdMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AstId {
    pub file: FileId,
    pub index: u32,
}

/// Per file: the declaration nodes in tree order, each reachable again
/// through its [`SyntaxNodePtr`]. The map holds pointers (kind plus
/// range), never red nodes, so it is a plain `Send + Sync` value — and
/// its value changes whenever ranges shift, which is why nothing
/// long-lived may depend on it (consumers sit behind the item tree and
/// reconcile spans through this map as late as possible).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AstIdMap {
    pointers: Vec<SyntaxNodePtr>,
}

impl AstIdMap {
    /// Numbers the declaration nodes of one file's syntax tree.
    pub fn from_root(root: &SyntaxNode) -> Self {
        Self {
            pointers: item_nodes(root)
                .iter()
                .map(|item| SyntaxNodePtr::new(&item.node))
                .collect(),
        }
    }

    /// The pointer of the declaration numbered `index`.
    pub fn pointer(&self, index: u32) -> Option<SyntaxNodePtr> {
        self.pointers.get(index as usize).copied()
    }

    /// The number of `node`, when it is a declaration node of the tree
    /// this map was built from.
    pub fn index_of(&self, node: &SyntaxNode) -> Option<u32> {
        let pointer = SyntaxNodePtr::new(node);
        self.pointers
            .iter()
            .position(|candidate| *candidate == pointer)
            .and_then(|position| u32::try_from(position).ok())
    }

    pub fn len(&self) -> usize {
        self.pointers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pointers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_syntax::SyntaxKind;

    use super::AstIdMap;

    fn map_of(source: &str) -> (celerrate_syntax::Parse, AstIdMap) {
        let parse = celerrate_syntax::parse(source);
        let map = AstIdMap::from_root(&parse.tree());
        (parse, map)
    }

    fn kinds_of(map: &AstIdMap) -> Vec<SyntaxKind> {
        (0..u32::try_from(map.len()).unwrap())
            .filter_map(|index| map.pointer(index))
            .map(|pointer| pointer.kind())
            .collect()
    }

    #[test]
    fn declaration_nodes_are_numbered_in_tree_order() {
        let (_parse, map) = map_of(
            "<?php namespace N; use A; function first() {} class Second {} const THIRD = 1;",
        );
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::NamespaceDeclaration,
                SyntaxKind::UseDeclaration,
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::ClassDeclaration,
                SyntaxKind::ConstantDeclaration,
            ],
        );
    }

    #[test]
    fn guarded_declarations_are_numbered() {
        // The section 7 stance: a symbol declared behind a guard counts
        // as declared, so the walk descends into blocks and bodies.
        let (_parse, map) = map_of(
            "<?php\n\
             if (!function_exists('greet')) { function greet() {} }\n\
             function outer() { function inner() {} }\n",
        );
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::FunctionDeclaration,
            ],
        );
    }

    #[test]
    fn members_are_not_declaration_nodes() {
        // Class constants, methods, and anything inside a member list
        // are members, not items; the constant after the class is one.
        let (_parse, map) =
            map_of("<?php class A { const B = 1; public function method() {} } const C = 1;");
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::ClassDeclaration,
                SyntaxKind::ConstantDeclaration
            ],
        );
    }

    #[test]
    fn nameless_declarations_are_not_numbered() {
        // Anonymous classes carry no top-level identity.
        let (_parse, map) =
            map_of("<?php function wrapper() { return new class {}; } class Named {}");
        assert_eq!(
            kinds_of(&map),
            vec![
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::ClassDeclaration
            ],
        );
    }

    #[test]
    fn a_pointer_reconciles_back_to_its_node() {
        let (parse, map) = map_of("<?php function greet() {}");
        let pointer = map.pointer(0).unwrap();
        let node = pointer.try_to_node(&parse.tree()).unwrap();
        assert_eq!(node.kind(), SyntaxKind::FunctionDeclaration);
        assert_eq!(map.index_of(&node), Some(0));
    }

    #[test]
    fn an_unknown_index_and_a_non_item_node_answer_none() {
        let (parse, map) = map_of("<?php function greet() {}");
        assert_eq!(map.pointer(99), None);
        assert_eq!(map.index_of(&parse.tree()), None);
        assert!(!map.is_empty());
    }

    #[test]
    fn a_body_edit_renumbers_nothing() {
        let (_before_parse, before) = map_of("<?php function a() { return 1; } class B {}");
        let (after_parse, after) = map_of("<?php function a() { return 1 + 1; } class B {}");
        // Class B moved (its range changed), but it kept its number.
        assert_eq!(
            before.pointer(1).unwrap().kind(),
            SyntaxKind::ClassDeclaration,
        );
        let class_node = after
            .pointer(1)
            .unwrap()
            .try_to_node(&after_parse.tree())
            .unwrap();
        assert_eq!(class_node.kind(), SyntaxKind::ClassDeclaration);
        assert_eq!(after.index_of(&class_node), Some(1));
    }

    #[test]
    fn an_empty_file_has_an_empty_map() {
        let (_parse, map) = map_of("");
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }
}
