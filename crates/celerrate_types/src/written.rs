//! Parsing the written form of a native PHP type: the token-joined
//! text the member `ItemTree` carries (`Foo\Bar|null`, `?Logger`,
//! `(A&B)|C`). Grammar only: names stay unresolved strings, keywords
//! are ordinary names until lowering. Tolerant: malformed text is
//! `None`, never a panic.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WrittenType {
    /// A (possibly qualified) name or keyword, exactly as written.
    Name(String),
    Nullable(Box<WrittenType>),
    Union(Vec<WrittenType>),
    Intersection(Vec<WrittenType>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Cap on `parse_atom` recursion depth (`parse_union` -> `parse_intersection`
/// -> `parse_atom`, plus `?T`'s direct self-recursion and `(...)`'s cycle
/// back through `parse_union`). Mirrors `norm.rs`'s `MAX_ATOM_NESTING_DEPTH`:
/// no legitimate written type nests anywhere close, so hostile input such as
/// `"(".repeat(100_000)` answers `None` instead of overflowing the stack.
const MAX_ATOM_NESTING_DEPTH: usize = 256;

struct Cursor<'a> {
    tokens: &'a [Token],
    position: usize,
    /// Current `parse_atom` call-stack depth. Incremented on entry and
    /// decremented on exit (see `parse_atom`), so it tracks live nesting
    /// rather than the total number of atoms parsed.
    depth: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) {
        self.position += 1;
    }

    fn at_end(&self) -> bool {
        self.position == self.tokens.len()
    }
}

pub(crate) fn parse_written(text: &str) -> Option<WrittenType> {
    let tokens = lex(text)?;
    let mut cursor = Cursor {
        tokens: &tokens,
        position: 0,
        depth: 0,
    };
    let parsed = parse_union(&mut cursor)?;
    cursor.at_end().then_some(parsed)
}

/// union := intersection (`|` intersection)*
fn parse_union(cursor: &mut Cursor<'_>) -> Option<WrittenType> {
    let mut parts = vec![parse_intersection(cursor)?];
    while cursor.peek() == Some(&Token::Pipe) {
        cursor.advance();
        parts.push(parse_intersection(cursor)?);
    }
    Some(if parts.len() == 1 {
        parts.remove(0)
    } else {
        WrittenType::Union(parts)
    })
}

/// intersection := atom (`&` atom)*
fn parse_intersection(cursor: &mut Cursor<'_>) -> Option<WrittenType> {
    let mut parts = vec![parse_atom(cursor)?];
    while cursor.peek() == Some(&Token::Ampersand) {
        cursor.advance();
        parts.push(parse_atom(cursor)?);
    }
    Some(if parts.len() == 1 {
        parts.remove(0)
    } else {
        WrittenType::Intersection(parts)
    })
}

/// Guards `parse_atom_body`'s recursion depth: hostile input answers
/// `None`, never crashes. See `MAX_ATOM_NESTING_DEPTH`.
fn parse_atom(cursor: &mut Cursor<'_>) -> Option<WrittenType> {
    cursor.depth += 1;
    let result = if cursor.depth > MAX_ATOM_NESTING_DEPTH {
        None
    } else {
        parse_atom_body(cursor)
    };
    cursor.depth -= 1;
    result
}

/// atom := `?` atom | `(` union `)` | name
fn parse_atom_body(cursor: &mut Cursor<'_>) -> Option<WrittenType> {
    match cursor.peek()? {
        Token::Question => {
            cursor.advance();
            Some(WrittenType::Nullable(Box::new(parse_atom(cursor)?)))
        }
        Token::OpenParenthesis => {
            cursor.advance();
            let inner = parse_union(cursor)?;
            if cursor.peek() != Some(&Token::CloseParenthesis) {
                return None;
            }
            cursor.advance();
            Some(inner)
        }
        Token::Name(name) => {
            let name = name.clone();
            cursor.advance();
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
    fn deeply_nested_input_answers_none_instead_of_overflowing_the_stack() {
        // Comfortably past `MAX_ATOM_NESTING_DEPTH`. Each of these alone
        // crashes the process (stack overflow) without the depth guard in
        // `parse_atom` — the same shape `norm.rs` guards. The written form
        // is derived from user-supplied source, so hostile nesting must
        // answer `None`, not a SIGSEGV.
        let deeply_nested_parentheses = "(".repeat(100_000);
        let deeply_nested_nullable = "?".repeat(100_000);
        assert_eq!(parse_written(&deeply_nested_parentheses), None);
        assert_eq!(parse_written(&deeply_nested_nullable), None);
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
