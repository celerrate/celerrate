use crate::lexer::{BASE_SCRIPTING, Lexer};
use crate::syntax_kind::SyntaxKind;

impl Lexer<'_> {
    pub(super) fn lex_inline_html(&mut self) {
        // A first-line shebang is trivia, before anything else.
        if u32::from(self.token_start()) == 0 && self.cursor.rest().starts_with("#!") {
            self.cursor
                .eat_while(|character| character != '\n' && character != '\r');
            self.emit(SyntaxKind::Shebang);
            return;
        }
        match self.cursor.rest().find("<?") {
            Some(0) => self.lex_open_tag(),
            Some(tag_position) => {
                self.cursor.bump_bytes(tag_position);
                self.emit(SyntaxKind::InlineHtml);
            }
            None => {
                self.cursor.bump_bytes(self.cursor.rest().len());
                self.emit(SyntaxKind::InlineHtml);
            }
        }
    }

    fn lex_open_tag(&mut self) {
        let rest = self.cursor.rest();
        if starts_with_full_open_tag(rest) {
            self.cursor.bump_bytes(5);
            self.emit(SyntaxKind::OpenTag);
        } else if rest.starts_with("<?=") {
            self.cursor.bump_bytes(3);
            self.emit(SyntaxKind::OpenTagEcho);
        } else {
            // The short tag is lexed unconditionally: its availability
            // depends on an ini setting, judged semantically upstairs.
            self.cursor.bump_bytes(2);
            self.emit(SyntaxKind::ShortOpenTag);
        }
        self.set_mode(BASE_SCRIPTING);
    }
}

/// `<?php` case-insensitively, followed by whitespace or end of input;
/// otherwise `<?phpx` must lex as a short tag plus scripting content.
fn starts_with_full_open_tag(rest: &str) -> bool {
    let Some(tag) = rest.get(..5) else {
        return false;
    };
    if !tag.eq_ignore_ascii_case("<?php") {
        return false;
    }
    matches!(
        rest.as_bytes().get(5),
        None | Some(b' ' | b'\t' | b'\n' | b'\r')
    )
}
