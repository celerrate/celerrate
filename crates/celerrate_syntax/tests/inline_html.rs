mod support;

use celerrate_syntax::SyntaxKind::*;
use support::{kinds, lex_verified, texts};

#[test]
fn empty_input_yields_no_tokens() {
    let (tokens, diagnostics) = lex_verified("");
    assert!(tokens.is_empty());
    assert!(diagnostics.is_empty());
}

#[test]
fn pure_html_is_one_inline_html_token() {
    assert_eq!(kinds("<h1>Hello</h1>"), [InlineHtml]);
}

#[test]
fn open_tag_after_html() {
    assert_eq!(
        texts("<div><?php"),
        [
            (InlineHtml, "<div>".to_owned()),
            (OpenTag, "<?php".to_owned()),
        ]
    );
}

#[test]
fn open_tag_is_case_insensitive() {
    assert_eq!(kinds("<?PHP"), [OpenTag]);
    assert_eq!(kinds("<?Php\n"), [OpenTag, Whitespace]);
}

#[test]
fn open_tag_requires_a_boundary() {
    // "<?phpx" is a short open tag followed by scripting content.
    let listing = texts("<?phpx");
    assert_eq!(listing.first(), Some(&(ShortOpenTag, "<?".to_owned())));
}

#[test]
fn echo_and_short_open_tags() {
    assert_eq!(kinds("<?="), [OpenTagEcho]);
    assert_eq!(kinds("<?"), [ShortOpenTag]);
}

#[test]
fn close_tag_returns_to_html_and_swallows_one_newline() {
    assert_eq!(
        texts("<?php ?>\nhtml\n<?php"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (CloseTag, "?>\n".to_owned()),
            (InlineHtml, "html\n".to_owned()),
            (OpenTag, "<?php".to_owned()),
        ]
    );
}

#[test]
fn close_tag_swallows_a_crlf_newline() {
    assert_eq!(
        texts("<?php ?>\r\nx"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (CloseTag, "?>\r\n".to_owned()),
            (InlineHtml, "x".to_owned()),
        ]
    );
}

#[test]
fn shebang_on_the_first_line_is_trivia() {
    assert_eq!(
        texts("#!/usr/bin/env php\n<?php"),
        [
            (Shebang, "#!/usr/bin/env php".to_owned()),
            (InlineHtml, "\n".to_owned()),
            (OpenTag, "<?php".to_owned()),
        ]
    );
}

#[test]
fn lone_angle_brackets_stay_inline_html() {
    assert_eq!(kinds("a < b <today>"), [InlineHtml]);
}
