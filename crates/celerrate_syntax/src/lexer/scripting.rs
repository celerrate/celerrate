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

/// A radix prefix counts only when a digit of that radix follows: "0x"
/// alone or "0xyz" lexes as the integer zero then a name, as in Zend.
fn starts_with_radix_prefix(rest: &str, prefix: &str, is_digit: impl Fn(char) -> bool) -> bool {
    rest.strip_prefix(prefix)
        .is_some_and(|after| after.starts_with(is_digit))
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
            character if character.is_ascii_digit() => self.lex_number(),
            '.' if self
                .cursor
                .peek_second()
                .is_some_and(|c| c.is_ascii_digit()) =>
            {
                self.lex_number()
            }
            character if is_name_start(character) => self.lex_name(),
            ';' => {
                self.cursor.eat(';');
                self.emit(SyntaxKind::Semicolon);
            }
            '+' => {
                self.cursor.eat('+');
                self.emit(SyntaxKind::Plus);
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

    fn lex_number(&mut self) {
        // Binary and octal deliberately take the maximal decimal-digit
        // run: digit validity ("0b2", "0o99") is judged upstairs, so each
        // stays a single literal.
        let rest = self.cursor.rest();
        let is_hex_digit = |c: char| c.is_ascii_hexdigit();
        let is_decimal_digit = |c: char| c.is_ascii_digit();
        if starts_with_radix_prefix(rest, "0x", is_hex_digit)
            || starts_with_radix_prefix(rest, "0X", is_hex_digit)
        {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_hexdigit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        if starts_with_radix_prefix(rest, "0b", is_decimal_digit)
            || starts_with_radix_prefix(rest, "0B", is_decimal_digit)
        {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        if starts_with_radix_prefix(rest, "0o", is_decimal_digit)
            || starts_with_radix_prefix(rest, "0O", is_decimal_digit)
        {
            self.cursor.bump_bytes(2);
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
            self.emit(SyntaxKind::IntegerLiteral);
            return;
        }
        // Decimal digits. Separator placement and octal digit validity
        // are judged upstairs; the lexer takes the maximal run.
        let mut is_float = false;
        self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
        if self.cursor.peek() == Some('.')
            && (self
                .cursor
                .peek_second()
                .is_some_and(|c| c.is_ascii_digit())
                || !self.cursor.pending_text().is_empty())
        {
            // "1.5", "1.", and ".5" are all floats, as in Zend's DNUM.
            is_float = true;
            self.cursor.eat('.');
            self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
        }
        if self.eat_exponent() {
            is_float = true;
        }
        let kind = if is_float {
            SyntaxKind::FloatLiteral
        } else {
            SyntaxKind::IntegerLiteral
        };
        self.emit(kind);
    }

    /// Consumes `[eE][+-]?digits` only when the digits are there;
    /// otherwise consumes nothing ("1e" is an integer then a name).
    fn eat_exponent(&mut self) -> bool {
        if !matches!(self.cursor.peek(), Some('e' | 'E')) {
            return false;
        }
        let after_marker = self.cursor.rest().get(1..).unwrap_or_default();
        let after_sign = after_marker
            .strip_prefix(['+', '-'])
            .unwrap_or(after_marker);
        if !after_sign.starts_with(|c: char| c.is_ascii_digit()) {
            return false;
        }
        self.cursor.bump();
        if matches!(self.cursor.peek(), Some('+' | '-')) {
            self.cursor.bump();
        }
        self.cursor.eat_while(|c| c.is_ascii_digit() || c == '_');
        true
    }
}
