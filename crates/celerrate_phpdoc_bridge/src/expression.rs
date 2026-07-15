//! The 4a standard type-expression grammar (decision 6):
//!
//! ```text
//! union        := intersection ('|' intersection)*
//! intersection := suffixed ('&' suffixed)*
//! suffixed     := atom ('[' ']')*
//! atom         := '?' suffixed | '(' union ')' | name
//! ```
//!
//! Anything outside this grammar — generics (`array<int, string>`),
//! shapes (`array{id: int}`), literals, `class-string<T>`, integer
//! ranges — is the PHPStan dialect (plan 4b) and answers `None` here:
//! loss is per construct, never per annotation.
//!
//! The parser is a recursive descent over a peekable char cursor with
//! a depth guard: adversarial nesting must not overflow the stack.
//! No `unwrap`, no indexing: only `chars().peekable()` and owned
//! strings.

use std::iter::Peekable;
use std::str::Chars;

/// A parsed standard-notation type expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpression {
    Name(String),
    Nullable(Box<TypeExpression>),
    Union(Vec<TypeExpression>),
    Intersection(Vec<TypeExpression>),
    ArrayOf(Box<TypeExpression>),
}

/// Nesting is refused past this depth: adversarial input (`(((((...`)
/// must not overflow the stack.
const MAXIMUM_DEPTH: u32 = 64;

/// Parses `text` as a standard-notation type expression. The whole
/// input must be consumed (trailing whitespace allowed); anything
/// left over, anything outside the grammar, or anything nested past
/// [`MAXIMUM_DEPTH`] answers `None`.
pub fn parse_type_expression_text(text: &str) -> Option<TypeExpression> {
    let mut cursor = text.chars().peekable();
    let expression = parse_union(&mut cursor, 0)?;
    skip_whitespace(&mut cursor);
    if cursor.next().is_some() {
        return None;
    }
    Some(expression)
}

fn skip_whitespace(cursor: &mut Peekable<Chars<'_>>) {
    while let Some(character) = cursor.peek() {
        if character.is_whitespace() {
            cursor.next();
        } else {
            break;
        }
    }
}

fn is_name_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '\\' || character >= '\u{80}'
}

fn parse_union(cursor: &mut Peekable<Chars<'_>>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut members = vec![parse_intersection(cursor, depth + 1)?];
    loop {
        skip_whitespace(cursor);
        let mut lookahead = cursor.clone();
        if lookahead.next() != Some('|') {
            break;
        }
        cursor.next();
        skip_whitespace(cursor);
        members.push(parse_intersection(cursor, depth + 1)?);
    }
    if members.len() == 1 {
        members.into_iter().next()
    } else {
        Some(TypeExpression::Union(members))
    }
}

fn parse_intersection(cursor: &mut Peekable<Chars<'_>>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut members = vec![parse_suffixed(cursor, depth + 1)?];
    loop {
        skip_whitespace(cursor);
        let mut lookahead = cursor.clone();
        if lookahead.next() != Some('&') {
            break;
        }
        cursor.next();
        skip_whitespace(cursor);
        members.push(parse_suffixed(cursor, depth + 1)?);
    }
    if members.len() == 1 {
        members.into_iter().next()
    } else {
        Some(TypeExpression::Intersection(members))
    }
}

fn parse_suffixed(cursor: &mut Peekable<Chars<'_>>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut expression = parse_atom(cursor, depth + 1)?;
    loop {
        skip_whitespace(cursor);
        let mut lookahead = cursor.clone();
        if lookahead.next() != Some('[') {
            break;
        }
        if lookahead.next() != Some(']') {
            break;
        }
        cursor.next();
        cursor.next();
        expression = TypeExpression::ArrayOf(Box::new(expression));
    }
    Some(expression)
}

fn parse_atom(cursor: &mut Peekable<Chars<'_>>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    skip_whitespace(cursor);
    match cursor.peek() {
        Some('?') => {
            cursor.next();
            skip_whitespace(cursor);
            let inner = parse_suffixed(cursor, depth + 1)?;
            Some(TypeExpression::Nullable(Box::new(inner)))
        }
        Some('(') => {
            cursor.next();
            skip_whitespace(cursor);
            let inner = parse_union(cursor, depth + 1)?;
            skip_whitespace(cursor);
            if cursor.next() != Some(')') {
                return None;
            }
            Some(inner)
        }
        Some(character) if is_name_character(*character) => parse_name(cursor),
        _ => None,
    }
}

fn parse_name(cursor: &mut Peekable<Chars<'_>>) -> Option<TypeExpression> {
    let mut name = String::new();
    while let Some(character) = cursor.peek() {
        if is_name_character(*character) {
            name.push(*character);
            cursor.next();
        } else {
            break;
        }
    }
    if name.is_empty() {
        None
    } else {
        Some(TypeExpression::Name(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_grammar_parses() {
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("int"),
            Some(Name("int".to_owned()))
        );
        assert_eq!(
            parse_type_expression_text("?string"),
            Some(Nullable(Box::new(Name("string".to_owned())))),
        );
        assert_eq!(
            parse_type_expression_text("int|null"),
            Some(Union(vec![Name("int".to_owned()), Name("null".to_owned())])),
        );
        assert_eq!(
            parse_type_expression_text("Countable&Traversable"),
            Some(Intersection(vec![
                Name("Countable".to_owned()),
                Name("Traversable".to_owned()),
            ])),
        );
        assert_eq!(
            parse_type_expression_text("User[]"),
            Some(ArrayOf(Box::new(Name("User".to_owned())))),
        );
        assert_eq!(
            parse_type_expression_text("(int|string)[]"),
            Some(ArrayOf(Box::new(Union(vec![
                Name("int".to_owned()),
                Name("string".to_owned()),
            ])))),
        );
        assert_eq!(
            parse_type_expression_text("\\App\\User"),
            Some(Name("\\App\\User".to_owned())),
        );
    }

    #[test]
    fn dialect_constructs_and_garbage_answer_none() {
        for text in [
            "array<int, string>",
            "array{id: int}",
            "class-string<T>",
            "'literal'",
            "int<1, max>",
            "",
            "|",
            "?",
            "int|",
            "((int)",
            "int string",
        ] {
            assert_eq!(parse_type_expression_text(text), None, "{text}");
        }
    }

    #[test]
    fn adversarial_expressions_never_panic() {
        let repeated = "a".repeat(10_000);
        for text in [
            "????",
            "(((((",
            "]][[",
            "\u{0}|\u{0}",
            "&&&",
            repeated.as_str(),
        ] {
            let _ = parse_type_expression_text(text);
        }
    }
}
