//! The dialect token stream over one tag's content. Tokens carry byte
//! offsets so the tag layer can slice a consumed prefix verbatim.
//! Tokenization is greedy and total: it stops at the first character
//! it cannot tokenize and returns the tokens it has — the prose after
//! a type expression is allowed to be untokenizable. `//` comments
//! (the pinned reference accepts them inside array shapes) are
//! skipped to the end of their line.

/// One token kind. Names capture PHP identifiers including `\`,
/// non-ASCII leading bytes, and interior hyphens (`class-string`,
/// `non-empty-array`); numbers capture an optional leading minus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Name(String),
    /// `$name`, including `$this`. The `$` is not stored.
    Variable(String),
    Integer(i64),
    /// Float literals keep their written text (the expression layer
    /// stays `Eq`; lowering parses). An integer literal that
    /// overflows `i64` also lands here: its lowering degrades, the
    /// tokenizer never fails on it.
    Float(String),
    StringLiteral(String),
    Pipe,
    Ampersand,
    Question,
    Comma,
    Colon,
    DoubleColon,
    Equals,
    OpenParenthesis,
    CloseParenthesis,
    OpenAngle,
    CloseAngle,
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Ellipsis,
    Asterisk,
}

/// One token with its byte span in the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

// Temporary: consumed by the token parser of the next task.
#[allow(dead_code)]
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut cursor = text.char_indices().peekable();
    while let Some(&(start, character)) = cursor.peek() {
        if character.is_whitespace() {
            cursor.next();
            continue;
        }
        // `//` comments run to the end of their line (the pinned
        // reference accepts them inside array shapes).
        if character == '/' && starts_with_at(text, start, "//") {
            while let Some(&(_, character)) = cursor.peek() {
                if character == '\n' {
                    break;
                }
                cursor.next();
            }
            continue;
        }
        let Some(token) = lex_token(text, &mut cursor, start, character) else {
            break;
        };
        tokens.push(token);
    }
    tokens
}

fn starts_with_at(text: &str, start: usize, prefix: &str) -> bool {
    text.get(start..)
        .is_some_and(|remainder| remainder.starts_with(prefix))
}

fn lex_token(
    text: &str,
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
    character: char,
) -> Option<Token> {
    let simple = |kind: TokenKind, width: usize| Token {
        kind,
        start,
        end: start + width,
    };
    match character {
        '|' => advance(cursor, 1).then(|| simple(TokenKind::Pipe, 1)),
        '&' => advance(cursor, 1).then(|| simple(TokenKind::Ampersand, 1)),
        '?' => advance(cursor, 1).then(|| simple(TokenKind::Question, 1)),
        ',' => advance(cursor, 1).then(|| simple(TokenKind::Comma, 1)),
        '=' => advance(cursor, 1).then(|| simple(TokenKind::Equals, 1)),
        '(' => advance(cursor, 1).then(|| simple(TokenKind::OpenParenthesis, 1)),
        ')' => advance(cursor, 1).then(|| simple(TokenKind::CloseParenthesis, 1)),
        '<' => advance(cursor, 1).then(|| simple(TokenKind::OpenAngle, 1)),
        '>' => advance(cursor, 1).then(|| simple(TokenKind::CloseAngle, 1)),
        '{' => advance(cursor, 1).then(|| simple(TokenKind::OpenBrace, 1)),
        '}' => advance(cursor, 1).then(|| simple(TokenKind::CloseBrace, 1)),
        '[' => advance(cursor, 1).then(|| simple(TokenKind::OpenBracket, 1)),
        ']' => advance(cursor, 1).then(|| simple(TokenKind::CloseBracket, 1)),
        '*' => advance(cursor, 1).then(|| simple(TokenKind::Asterisk, 1)),
        ':' => {
            if starts_with_at(text, start, "::") {
                advance(cursor, 2).then(|| simple(TokenKind::DoubleColon, 2))
            } else {
                advance(cursor, 1).then(|| simple(TokenKind::Colon, 1))
            }
        }
        '.' => {
            if starts_with_at(text, start, "...") {
                advance(cursor, 3).then(|| simple(TokenKind::Ellipsis, 3))
            } else {
                None
            }
        }
        '$' => lex_variable(cursor, start),
        '\'' | '"' => lex_string(cursor, start, character),
        '-' => {
            let mut lookahead = cursor.clone();
            lookahead.next();
            match lookahead.peek() {
                Some(&(_, digit)) if digit.is_ascii_digit() => lex_number(text, cursor, start),
                _ => None,
            }
        }
        character if character.is_ascii_digit() => lex_number(text, cursor, start),
        character if is_name_start(character) => lex_name(cursor, start),
        _ => None,
    }
}

/// Advances the cursor by `count` characters; answers `true` so the
/// caller can chain with `.then()`.
fn advance(cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>, count: usize) -> bool {
    for _ in 0..count {
        cursor.next();
    }
    true
}

fn is_name_start(character: char) -> bool {
    character.is_alphabetic() || character == '_' || character == '\\' || character >= '\u{80}'
}

fn is_name_continue(character: char) -> bool {
    character.is_alphanumeric()
        || character == '_'
        || character == '\\'
        || character == '-'
        || character >= '\u{80}'
}

fn lex_name(
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
) -> Option<Token> {
    let mut name = String::new();
    let mut end = start;
    while let Some(&(offset, character)) = cursor.peek() {
        let accepted = if name.is_empty() {
            is_name_start(character)
        } else {
            is_name_continue(character)
        };
        if !accepted {
            break;
        }
        name.push(character);
        end = offset + character.len_utf8();
        cursor.next();
    }
    // A trailing hyphen belongs to prose (`foo- bar`), not the name:
    // give it back so the parser never sees `foo-` as one identifier.
    while name.ends_with('-') {
        name.pop();
        end -= 1;
    }
    if name.is_empty() {
        None
    } else {
        Some(Token {
            kind: TokenKind::Name(name),
            start,
            end,
        })
    }
}

fn lex_variable(
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
) -> Option<Token> {
    cursor.next(); // the `$`
    let mut name = String::new();
    let mut end = start + 1;
    while let Some(&(offset, character)) = cursor.peek() {
        if character.is_ascii_alphanumeric() || character == '_' {
            name.push(character);
            end = offset + character.len_utf8();
            cursor.next();
        } else {
            break;
        }
    }
    if name.is_empty() {
        None
    } else {
        Some(Token {
            kind: TokenKind::Variable(name),
            start,
            end,
        })
    }
}

fn lex_string(
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
    quote: char,
) -> Option<Token> {
    cursor.next(); // the opening quote
    let mut value = String::new();
    while let Some((offset, character)) = cursor.next() {
        if character == quote {
            return Some(Token {
                kind: TokenKind::StringLiteral(value),
                start,
                end: offset + character.len_utf8(),
            });
        }
        if character == '\\' {
            let (_, escaped) = cursor.next()?;
            match escaped {
                '\\' => value.push('\\'),
                escaped if escaped == quote => value.push(quote),
                'n' if quote == '"' => value.push('\n'),
                't' if quote == '"' => value.push('\t'),
                'r' if quote == '"' => value.push('\r'),
                // PHP single-quote semantics: any other escape keeps
                // both characters.
                other => {
                    value.push('\\');
                    value.push(other);
                }
            }
        } else {
            value.push(character);
        }
    }
    None // unterminated: the construct stops here
}

fn lex_number(
    text: &str,
    cursor: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
) -> Option<Token> {
    let mut written = String::new();
    let mut end = start;
    let mut is_float = false;
    if let Some(&(_, '-')) = cursor.peek() {
        written.push('-');
        end += 1;
        cursor.next();
    }
    let radix_prefix = ["0x", "0X", "0b", "0B", "0o", "0O"]
        .iter()
        .find(|prefix| starts_with_at(text, end, prefix))
        .copied();
    if let Some(prefix) = radix_prefix {
        written.push_str(prefix);
        end += 2;
        advance(cursor, 2);
    }
    let mut digits = String::new();
    while let Some(&(offset, character)) = cursor.peek() {
        let accepted = match radix_prefix {
            Some("0x" | "0X") => character.is_ascii_hexdigit() || character == '_',
            Some(_) => character.is_ascii_digit() || character == '_',
            None => {
                character.is_ascii_digit()
                    || character == '_'
                    || character == '.'
                    || character == 'e'
                    || character == 'E'
                    || ((character == '+' || character == '-')
                        && (digits.ends_with('e') || digits.ends_with('E')))
            }
        };
        if !accepted {
            break;
        }
        if character == '.' {
            // `..` would be an ellipsis after an integer, not a float
            // dot: only consume a dot followed by a digit.
            let mut lookahead = cursor.clone();
            lookahead.next();
            if !matches!(lookahead.peek(), Some(&(_, next)) if next.is_ascii_digit()) {
                break;
            }
            is_float = true;
        }
        if character == 'e' || character == 'E' {
            is_float = true;
        }
        digits.push(character);
        written.push(character);
        end = offset + character.len_utf8();
        cursor.next();
    }
    if digits.trim_matches('_').is_empty() {
        return None;
    }
    let kind = if is_float {
        TokenKind::Float(written)
    } else {
        let cleaned: String = written
            .chars()
            .filter(|&character| character != '_')
            .collect();
        let parsed = match radix_prefix {
            Some("0x" | "0X") => i64::from_str_radix(
                cleaned.trim_start_matches("0x").trim_start_matches("0X"),
                16,
            ),
            Some("0b" | "0B") => {
                i64::from_str_radix(cleaned.trim_start_matches("0b").trim_start_matches("0B"), 2)
            }
            Some(_) => {
                i64::from_str_radix(cleaned.trim_start_matches("0o").trim_start_matches("0O"), 8)
            }
            None => cleaned.parse::<i64>(),
        };
        match parsed {
            Ok(value) => TokenKind::Integer(value),
            // Beyond i64: degrade to Float text; lowering widens.
            Err(_) => TokenKind::Float(written),
        }
    };
    Some(Token { kind, start, end })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<TokenKind> {
        tokenize(text).into_iter().map(|token| token.kind).collect()
    }

    #[test]
    fn names_capture_backslashes_hyphens_and_unicode() {
        use TokenKind::*;
        assert_eq!(
            kinds("\\App\\User class-string non-empty-array Café"),
            vec![
                Name("\\App\\User".to_owned()),
                Name("class-string".to_owned()),
                Name("non-empty-array".to_owned()),
                Name("Café".to_owned()),
            ],
        );
    }

    #[test]
    fn punctuation_tokenizes_including_multi_character_forms() {
        use TokenKind::*;
        assert_eq!(
            kinds("|&?,:()<>{}[]=...*::"),
            vec![
                Pipe,
                Ampersand,
                Question,
                Comma,
                Colon,
                OpenParenthesis,
                CloseParenthesis,
                OpenAngle,
                CloseAngle,
                OpenBrace,
                CloseBrace,
                OpenBracket,
                CloseBracket,
                Equals,
                Ellipsis,
                Asterisk,
                DoubleColon,
            ],
        );
    }

    #[test]
    fn numbers_tokenize_with_sign_separators_radix_and_floats() {
        use TokenKind::*;
        assert_eq!(
            kinds("42 -1 1_000 0x7F 0b0110 0o777 1.5 -2.5e3"),
            vec![
                Integer(42),
                Integer(-1),
                Integer(1_000),
                Integer(0x7F),
                Integer(0b0110),
                Integer(0o777),
                Float("1.5".to_owned()),
                Float("-2.5e3".to_owned()),
            ],
        );
        // An integer beyond i64 degrades to a Float token, never an error.
        assert_eq!(
            kinds("99999999999999999999"),
            vec![Float("99999999999999999999".to_owned())],
        );
    }

    #[test]
    fn strings_tokenize_with_escapes_per_quote_kind() {
        use TokenKind::*;
        assert_eq!(
            kinds(r"'it\'s' 'a\\b'"),
            vec![
                StringLiteral("it's".to_owned()),
                StringLiteral("a\\b".to_owned()),
            ],
        );
        assert_eq!(
            kinds("\"a\\\"b\\n\""),
            vec![StringLiteral("a\"b\n".to_owned())],
        );
    }

    #[test]
    fn variables_and_comments_tokenize() {
        use TokenKind::*;
        assert_eq!(
            kinds("$this $items // trailing noise\n$next"),
            vec![
                Variable("this".to_owned()),
                Variable("items".to_owned()),
                Variable("next".to_owned()),
            ],
        );
    }

    #[test]
    fn tokenization_stops_at_the_first_untokenizable_character() {
        // `'` opens an unterminated string: everything before it
        // survives, nothing after it is invented.
        use TokenKind::*;
        assert_eq!(
            kinds("int $id the identifier isn't typed"),
            vec![
                Name("int".to_owned()),
                Variable("id".to_owned()),
                Name("the".to_owned()),
                Name("identifier".to_owned()),
                Name("isn".to_owned()),
            ],
        );
    }

    #[test]
    fn offsets_reconstruct_the_consumed_prefix() {
        let text = "array{id: int} $x";
        let tokens = tokenize(text);
        let close_brace = tokens
            .iter()
            .find(|token| token.kind == TokenKind::CloseBrace)
            .unwrap();
        assert_eq!(text.get(..close_brace.end), Some("array{id: int}"));
    }

    #[test]
    fn adversarial_inputs_never_panic() {
        let repeated = "?".repeat(10_000);
        for text in [
            "",
            "'",
            "\"",
            "\\",
            "$",
            "-",
            ".",
            "..",
            "0x",
            "1_",
            "\u{0}::\u{0}",
            repeated.as_str(),
        ] {
            let _ = tokenize(text);
        }
    }
}
