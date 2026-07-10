use crate::lexer::{Lexer, Mode};
use crate::syntax_kind::SyntaxKind;

/// PHP name start: `[a-zA-Z_\x80-\xff]`. Any non-ASCII char qualifies,
/// matching Zend's byte-oriented rule on UTF-8 input.
pub(crate) fn is_name_start(character: char) -> bool {
    character == '_' || character.is_ascii_alphabetic() || !character.is_ascii()
}

pub(crate) fn is_name_continue(character: char) -> bool {
    is_name_start(character) || character.is_ascii_digit()
}

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
            '$' => self.lex_dollar(),
            character if is_name_start(character) => self.lex_name(),
            ';' => {
                self.cursor.eat(';');
                self.emit(SyntaxKind::Semicolon);
            }
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

    fn lex_dollar(&mut self) {
        self.cursor.eat('$');
        if self.cursor.peek().is_some_and(is_name_start) {
            self.cursor.eat_while(is_name_continue);
            self.emit(SyntaxKind::Variable);
        } else {
            // `$$name` and a lone `$`: the dollar is its own token.
            self.emit(SyntaxKind::Dollar);
        }
    }

    fn lex_name(&mut self) {
        self.cursor.eat_while(is_name_continue);
        let kind =
            SyntaxKind::from_keyword(self.cursor.pending_text()).unwrap_or(SyntaxKind::Identifier);
        self.emit(kind);
    }
}
