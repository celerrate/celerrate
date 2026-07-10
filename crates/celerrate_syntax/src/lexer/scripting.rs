use crate::diagnostic::LexerDiagnosticKind;
use crate::lexer::{BASE_SCRIPTING, Lexer, Mode};
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
            '\'' => self.lex_single_quoted_string(),
            'b' | 'B' if self.cursor.peek_second() == Some('\'') => {
                self.cursor.bump();
                self.lex_single_quoted_string();
            }
            '"' => self.lex_double_quote_delimiter(),
            'b' | 'B' if self.cursor.peek_second() == Some('"') => {
                self.cursor.bump();
                self.lex_double_quote_delimiter();
            }
            '`' => self.lex_backtick_delimiter(),
            character if is_name_start(character) => self.lex_name(),
            '(' => self.lex_parenthesis_or_cast(),
            '{' => self.lex_open_brace(),
            '}' => self.lex_close_brace(),
            '/' if self.cursor.rest().starts_with("//") => self.lex_line_comment(),
            '/' if self.cursor.rest().starts_with("/*") => self.lex_block_comment(),
            '#' if self.cursor.rest().starts_with("#[") => {
                self.cursor.bump_bytes(2);
                self.emit(SyntaxKind::AttributeOpen);
            }
            '#' => self.lex_line_comment(),
            _ if self.try_lex_operator() => {}
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

    /// Every `{` pushes a scripting mode and every `}` pops one, exactly
    /// like Zend's state stack. Balanced braces are a no-op; the payoff
    /// is `{$expr}` interpolation, whose closing brace pops back into
    /// the string mode with no extra bookkeeping.
    fn lex_open_brace(&mut self) {
        self.cursor.eat('{');
        self.emit(SyntaxKind::OpenBrace);
        self.push_mode(BASE_SCRIPTING);
    }

    fn lex_close_brace(&mut self) {
        self.cursor.eat('}');
        self.emit(SyntaxKind::CloseBrace);
        if self.can_pop_mode() {
            self.pop_mode();
        }
    }

    fn lex_parenthesis_or_cast(&mut self) {
        if let Some((kind, byte_length)) = cast_at(self.cursor.rest()) {
            self.cursor.bump_bytes(byte_length);
            self.emit(kind);
        } else {
            self.cursor.eat('(');
            self.emit(SyntaxKind::OpenParenthesis);
        }
    }

    /// Longest-match scan of the operator table. Returns false when no
    /// operator starts here, letting the fallback take over.
    fn try_lex_operator(&mut self) -> bool {
        let rest = self.cursor.rest();
        for (text, kind) in OPERATORS {
            if rest.starts_with(text) {
                self.cursor.bump_bytes(text.len());
                self.emit(*kind);
                return true;
            }
        }
        false
    }

    /// `//` and `#` comments end before the newline, and also before a
    /// `?>` (the close tag still closes inside a line comment, as in
    /// Zend).
    fn lex_line_comment(&mut self) {
        while let Some(character) = self.cursor.peek() {
            if character == '\n' || character == '\r' {
                break;
            }
            if self.cursor.rest().starts_with("?>") {
                break;
            }
            self.cursor.bump();
        }
        self.emit(SyntaxKind::LineComment);
    }

    /// `/* ... */`, and `/** ... */` as a docblock when whitespace
    /// follows the doc opener (Zend's rule, which keeps "/**/" a plain
    /// comment). Unterminated comments run to the end of input with a
    /// diagnostic pointing at the opener.
    fn lex_block_comment(&mut self) {
        let start = self.token_start();
        let rest = self.cursor.rest();
        let is_docblock = rest
            .strip_prefix("/**")
            .is_some_and(|after| after.starts_with(|c: char| c.is_ascii_whitespace()));
        self.cursor.bump_bytes(2);
        match self.cursor.rest().find("*/") {
            Some(terminator_position) => {
                self.cursor.bump_bytes(terminator_position + 2);
            }
            None => {
                self.cursor.bump_bytes(self.cursor.rest().len());
                self.diagnose_at(LexerDiagnosticKind::UnterminatedBlockComment, start, 2);
            }
        }
        let kind = if is_docblock {
            SyntaxKind::DocComment
        } else {
            SyntaxKind::BlockComment
        };
        self.emit(kind);
    }
}

/// Operators and punctuation, longest first so prefixes never shadow a
/// longer operator. A linear scan is fine for now; a first-character
/// match tree can come with the benchmark part if it ever shows up.
const OPERATORS: &[(&str, SyntaxKind)] = &[
    ("<=>", SyntaxKind::Spaceship),
    ("===", SyntaxKind::EqualsEqualsEquals),
    ("!==", SyntaxKind::BangEqualsEquals),
    ("**=", SyntaxKind::StarStarEquals),
    ("<<=", SyntaxKind::LessLessEquals),
    (">>=", SyntaxKind::GreaterGreaterEquals),
    ("??=", SyntaxKind::QuestionQuestionEquals),
    ("...", SyntaxKind::Ellipsis),
    ("?->", SyntaxKind::NullsafeArrow),
    ("**", SyntaxKind::StarStar),
    ("==", SyntaxKind::EqualsEquals),
    ("!=", SyntaxKind::BangEquals),
    ("<>", SyntaxKind::BangEquals),
    ("<=", SyntaxKind::LessEquals),
    (">=", SyntaxKind::GreaterEquals),
    ("&&", SyntaxKind::AmpersandAmpersand),
    ("||", SyntaxKind::PipePipe),
    ("??", SyntaxKind::QuestionQuestion),
    ("++", SyntaxKind::PlusPlus),
    ("--", SyntaxKind::MinusMinus),
    ("<<", SyntaxKind::LessLess),
    (">>", SyntaxKind::GreaterGreater),
    ("+=", SyntaxKind::PlusEquals),
    ("-=", SyntaxKind::MinusEquals),
    ("*=", SyntaxKind::StarEquals),
    ("/=", SyntaxKind::SlashEquals),
    (".=", SyntaxKind::DotEquals),
    ("%=", SyntaxKind::PercentEquals),
    ("&=", SyntaxKind::AmpersandEquals),
    ("|=", SyntaxKind::PipeEquals),
    ("^=", SyntaxKind::CaretEquals),
    ("->", SyntaxKind::Arrow),
    ("=>", SyntaxKind::FatArrow),
    ("::", SyntaxKind::ColonColon),
    ("+", SyntaxKind::Plus),
    ("-", SyntaxKind::Minus),
    ("*", SyntaxKind::Star),
    ("/", SyntaxKind::Slash),
    ("%", SyntaxKind::Percent),
    ("=", SyntaxKind::Equals),
    ("<", SyntaxKind::Less),
    (">", SyntaxKind::Greater),
    ("!", SyntaxKind::Bang),
    ("&", SyntaxKind::Ampersand),
    ("|", SyntaxKind::Pipe),
    ("^", SyntaxKind::Caret),
    ("?", SyntaxKind::Question),
    (":", SyntaxKind::Colon),
    (";", SyntaxKind::Semicolon),
    (",", SyntaxKind::Comma),
    (".", SyntaxKind::Dot),
    ("@", SyntaxKind::At),
    ("~", SyntaxKind::Tilde),
    ("\\", SyntaxKind::Backslash),
    (")", SyntaxKind::CloseParenthesis),
    ("[", SyntaxKind::OpenBracket),
    ("]", SyntaxKind::CloseBracket),
];

/// Detects a cast at the start of `rest`: `(`, optional spaces and tabs,
/// one of the exact PHP 8.1 cast words (case-insensitive), optional
/// spaces and tabs, `)`. Returns the kind and the total byte length.
/// `(real)` and `(unset)` were removed in PHP 8.0 and do not match.
fn cast_at(rest: &str) -> Option<(SyntaxKind, usize)> {
    let inner = rest.strip_prefix('(')?;
    let after_leading = inner.trim_start_matches([' ', '\t']);
    let word_length = after_leading
        .chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .count();
    let word = after_leading.get(..word_length)?;
    let after_word = after_leading.get(word_length..)?;
    let after_trailing = after_word.trim_start_matches([' ', '\t']);
    after_trailing.strip_prefix(')')?;
    let kind = cast_kind(word)?;
    let total_length = rest.len() - after_trailing.len() + ')'.len_utf8();
    Some((kind, total_length))
}

fn cast_kind(word: &str) -> Option<SyntaxKind> {
    const CASTS: &[(&str, SyntaxKind)] = &[
        ("int", SyntaxKind::IntCast),
        ("integer", SyntaxKind::IntCast),
        ("bool", SyntaxKind::BoolCast),
        ("boolean", SyntaxKind::BoolCast),
        ("float", SyntaxKind::FloatCast),
        ("double", SyntaxKind::FloatCast),
        ("string", SyntaxKind::StringCast),
        ("binary", SyntaxKind::BinaryCast),
        ("array", SyntaxKind::ArrayCast),
        ("object", SyntaxKind::ObjectCast),
    ];
    CASTS
        .iter()
        .find(|(name, _)| word.eq_ignore_ascii_case(name))
        .map(|(_, kind)| *kind)
}
