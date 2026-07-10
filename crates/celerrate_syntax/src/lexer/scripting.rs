use crate::lexer::{Lexer, Mode};
use crate::syntax_kind::SyntaxKind;

impl Lexer<'_> {
    pub(super) fn lex_scripting(&mut self) {
        let Some(character) = self.cursor.peek() else {
            return;
        };
        match character {
            character if character.is_ascii_whitespace() => {
                self.cursor
                    .eat_while(|character| character.is_ascii_whitespace());
                self.emit(SyntaxKind::Whitespace);
            }
            '?' if self.cursor.rest().starts_with("?>") => self.lex_close_tag(),
            _ => self.lex_unexpected_character(),
        }
    }

    fn lex_close_tag(&mut self) {
        self.cursor.bump_bytes(2);
        // PHP swallows one newline right after `?>`; it belongs to the
        // close tag token so the stream stays lossless.
        if self.cursor.rest().starts_with("\r\n") {
            self.cursor.bump_bytes(2);
        } else {
            self.cursor.eat('\n');
        }
        self.emit(SyntaxKind::CloseTag);
        self.set_mode(Mode::InlineHtml);
    }
}
