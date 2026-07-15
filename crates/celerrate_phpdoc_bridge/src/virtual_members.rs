//! The bridge as a virtual-symbol provider: `@property` (and its
//! read/write variants) and `@method` declare members that exist for
//! the unknown-members family. Payload text stays unresolved — it
//! types downstream through the type-syntax point.

use celerrate_plugin::{VirtualMember, VirtualSymbolProvider};

use crate::lexer::lex_docblock;
use crate::syntax::PhpdocBridge;
use crate::tags::extract_virtual_members;

impl VirtualSymbolProvider for PhpdocBridge {
    fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
        extract_virtual_members(&lex_docblock(class_docblock))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_contributes_property_and_method_members() {
        let bridge = PhpdocBridge::new();
        let members = bridge.virtual_members(
            "/**\n * @property string $title\n * @method static User find(int $id)\n */",
        );
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "title");
        assert_eq!(members[1].name, "find");
    }

    #[test]
    fn a_docblock_without_virtual_tags_contributes_nothing() {
        let bridge = PhpdocBridge::new();
        assert!(bridge.virtual_members("/** @return int */").is_empty());
    }
}
