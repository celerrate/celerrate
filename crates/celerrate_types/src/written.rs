//! Parsing the written form of a native PHP type: the token-joined
//! text the member `ItemTree` carries (`Foo\Bar|null`, `?Logger`,
//! `(A&B)|C`). Grammar only: names stay unresolved strings, keywords
//! are ordinary names until lowering. Tolerant: malformed text is
//! `None`, never a panic.

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum WrittenType {
    /// A (possibly qualified) name or keyword, exactly as written.
    Name(String),
    Nullable(Box<WrittenType>),
    Union(Vec<WrittenType>),
    Intersection(Vec<WrittenType>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum Token {
    Name(String),
    Question,
    Pipe,
    Ampersand,
    OpenParenthesis,
    CloseParenthesis,
}

/// Lexes the joined text. `None` on any byte that cannot start or
/// continue a token (whitespace included: the joined form never
/// contains any).
#[allow(dead_code)]
fn lex(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(&character) = characters.peek() {
        match character {
            '?' => {
                characters.next();
                tokens.push(Token::Question);
            }
            '|' => {
                characters.next();
                tokens.push(Token::Pipe);
            }
            '&' => {
                characters.next();
                tokens.push(Token::Ampersand);
            }
            '(' => {
                characters.next();
                tokens.push(Token::OpenParenthesis);
            }
            ')' => {
                characters.next();
                tokens.push(Token::CloseParenthesis);
            }
            _ => tokens.push(Token::Name(lex_name(&mut characters)?)),
        }
    }
    Some(tokens)
}

/// One (possibly qualified, possibly `\`-prefixed) name. PHP labels
/// start with a letter, underscore, or a byte ≥ 0x80; digits may only
/// continue a label. A trailing `\` or an empty segment is malformed.
#[allow(dead_code)]
fn lex_name(characters: &mut core::iter::Peekable<core::str::Chars<'_>>) -> Option<String> {
    let mut name = String::new();
    if characters.peek() == Some(&'\\') {
        characters.next();
        name.push('\\');
    }
    loop {
        let mut segment = String::new();
        while let Some(&character) = characters.peek() {
            let continues =
                character.is_ascii_alphanumeric() || character == '_' || !character.is_ascii();
            if !continues {
                break;
            }
            if segment.is_empty() && character.is_ascii_digit() {
                return None;
            }
            segment.push(character);
            characters.next();
        }
        if segment.is_empty() {
            return None;
        }
        name.push_str(&segment);
        if characters.peek() == Some(&'\\') {
            characters.next();
            name.push('\\');
        } else {
            return Some(name);
        }
    }
}

#[allow(dead_code)]
pub(crate) fn parse_written(text: &str) -> Option<WrittenType> {
    let tokens = lex(text)?;
    let mut cursor = 0usize;
    let parsed = parse_union(&tokens, &mut cursor)?;
    (cursor == tokens.len()).then_some(parsed)
}

/// union := intersection (`|` intersection)*
#[allow(dead_code)]
fn parse_union(tokens: &[Token], cursor: &mut usize) -> Option<WrittenType> {
    let mut parts = vec![parse_intersection(tokens, cursor)?];
    while tokens.get(*cursor) == Some(&Token::Pipe) {
        *cursor += 1;
        parts.push(parse_intersection(tokens, cursor)?);
    }
    Some(if parts.len() == 1 {
        parts.remove(0)
    } else {
        WrittenType::Union(parts)
    })
}

/// intersection := atom (`&` atom)*
#[allow(dead_code)]
fn parse_intersection(tokens: &[Token], cursor: &mut usize) -> Option<WrittenType> {
    let mut parts = vec![parse_atom(tokens, cursor)?];
    while tokens.get(*cursor) == Some(&Token::Ampersand) {
        *cursor += 1;
        parts.push(parse_atom(tokens, cursor)?);
    }
    Some(if parts.len() == 1 {
        parts.remove(0)
    } else {
        WrittenType::Intersection(parts)
    })
}

/// atom := `?` atom | `(` union `)` | name
#[allow(dead_code)]
fn parse_atom(tokens: &[Token], cursor: &mut usize) -> Option<WrittenType> {
    match tokens.get(*cursor)? {
        Token::Question => {
            *cursor += 1;
            Some(WrittenType::Nullable(Box::new(parse_atom(tokens, cursor)?)))
        }
        Token::OpenParenthesis => {
            *cursor += 1;
            let inner = parse_union(tokens, cursor)?;
            if tokens.get(*cursor) != Some(&Token::CloseParenthesis) {
                return None;
            }
            *cursor += 1;
            Some(inner)
        }
        Token::Name(name) => {
            let name = name.clone();
            *cursor += 1;
            Some(WrittenType::Name(name))
        }
        Token::Pipe | Token::Ampersand | Token::CloseParenthesis => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{WrittenType, parse_written};

    fn name(text: &str) -> WrittenType {
        WrittenType::Name(text.to_owned())
    }

    #[test]
    fn a_plain_name_parses() {
        assert_eq!(parse_written("int"), Some(name("int")));
        assert_eq!(parse_written("Foo\\Bar"), Some(name("Foo\\Bar")));
        assert_eq!(parse_written("\\DateTime"), Some(name("\\DateTime")));
    }

    #[test]
    fn nullable_unions_and_intersections_parse() {
        assert_eq!(
            parse_written("?Logger"),
            Some(WrittenType::Nullable(Box::new(name("Logger")))),
        );
        assert_eq!(
            parse_written("Foo\\Bar|null"),
            Some(WrittenType::Union(vec![name("Foo\\Bar"), name("null")])),
        );
        assert_eq!(
            parse_written("Countable&Iterator"),
            Some(WrittenType::Intersection(vec![
                name("Countable"),
                name("Iterator"),
            ])),
        );
    }

    #[test]
    fn disjunctive_normal_form_parses_with_parentheses() {
        assert_eq!(
            parse_written("(A&B)|C"),
            Some(WrittenType::Union(vec![
                WrittenType::Intersection(vec![name("A"), name("B")]),
                name("C"),
            ])),
        );
    }

    #[test]
    fn unions_flatten_across_their_own_nesting() {
        // `A|B|C` is one three-part union, not a nested pair.
        assert_eq!(
            parse_written("A|B|C"),
            Some(WrittenType::Union(vec![name("A"), name("B"), name("C")])),
        );
    }

    #[test]
    fn malformed_text_is_none_never_a_panic() {
        for garbage in [
            "", "|", "?", "(", ")", "A|", "|A", "A&", "?(", "((A)", "A B", "A||B", "1nt", "\\",
            "A\\", "?|A", "A(B)",
        ] {
            assert_eq!(parse_written(garbage), None, "input {garbage:?}");
        }
    }

    #[test]
    fn every_ascii_soup_is_parsed_or_rejected_without_panicking() {
        // A cheap fuzz floor: three-byte soups over a hostile alphabet.
        let alphabet = b"A?|&()\\1_ ";
        for a in alphabet {
            for b in alphabet {
                for c in alphabet {
                    let text: String = [*a, *b, *c].iter().map(|&byte| byte as char).collect();
                    let _ = parse_written(&text);
                }
            }
        }
    }
}
