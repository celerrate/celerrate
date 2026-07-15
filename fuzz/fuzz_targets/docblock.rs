#![no_main]

use celerrate_plugin::CommentKind;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let tags = celerrate_phpdoc_bridge::lex_docblock(text);
        let _ = celerrate_phpdoc_bridge::extract_member_docblock(&tags);
        let _ = celerrate_phpdoc_bridge::extract_virtual_members(&tags);
        let _ = celerrate_phpdoc_bridge::parse_type_expression_text(text);
        for kind in [CommentKind::Line, CommentKind::Block, CommentKind::Docblock] {
            let _ = celerrate_phpdoc_bridge::comment_directives(kind, text);
        }
    }
});
