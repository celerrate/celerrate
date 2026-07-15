//! The inherited PHPDoc type-expression grammar (decision 6):
//!
//! ```text
//! union        := intersection ('|' intersection)*
//! intersection := suffixed ('&' suffixed)*
//! suffixed     := atom ('[' ']')*
//! atom         := '?' atom | '(' union ')' | name
//! ```
//!
//! Anything outside this grammar — generics (`array<int, string>`),
//! shapes (`array{id: int}`), literals, `class-string<T>`, integer
//! ranges — answers `None` here: loss is per construct, never per annotation.
//!
//! The parser is a recursive descent over the token stream (Task 1) with
//! a depth guard: adversarial nesting must not overflow the stack.
//! Entry points `parse_type_expression_text` consumes a whole input;
//! `parse_type_expression_prefix` reports the consumed byte length so
//! the tag layer can split type from prose.

mod parser;
mod tokens;

/// A parsed type expression of the inherited PHPDoc dialect family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpression {
    Name(String),
    Nullable(Box<TypeExpression>),
    Union(Vec<TypeExpression>),
    Intersection(Vec<TypeExpression>),
    ArrayOf(Box<TypeExpression>),
}

/// Parses `text` as one type expression consuming the whole input
/// (trailing whitespace allowed); anything left over, anything outside
/// the grammar, or anything nested past the depth guard answers
/// `None`.
pub fn parse_type_expression_text(text: &str) -> Option<TypeExpression> {
    let (expression, consumed) = parse_type_expression_prefix(text)?;
    let remainder = text.get(consumed..)?;
    if remainder.trim().is_empty() {
        Some(expression)
    } else {
        None
    }
}

/// Parses a maximal well-formed type expression from the start of
/// `text` and reports the consumed byte length — the tag layer takes
/// the type from the prefix and the variable or prose from the
/// remainder. Grammar failure anywhere answers `None` for the whole
/// expression: loss is per construct, never partially recovered.
pub fn parse_type_expression_prefix(text: &str) -> Option<(TypeExpression, usize)> {
    let tokens = tokens::tokenize(text);
    let mut cursor = parser::Parser::new(&tokens);
    let expression = parser::parse_type(&mut cursor, 0)?;
    let consumed = cursor.consumed_end()?;
    Some((expression, consumed))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn nullable_binds_inside_the_array_suffix() {
        // The reference parses `?int[]` as an array of nullable int
        // ((?int)[]), not a nullable array — decision 4.
        use TypeExpression::*;
        assert_eq!(
            parse_type_expression_text("?int[]"),
            Some(ArrayOf(Box::new(Nullable(Box::new(Name(
                "int".to_owned()
            )))))),
        );
    }

    #[test]
    fn prefix_parsing_reports_the_consumed_length() {
        let (expression, consumed) =
            parse_type_expression_prefix("int|string $x the identifier").unwrap();
        assert_eq!(
            expression,
            TypeExpression::Union(vec![
                TypeExpression::Name("int".to_owned()),
                TypeExpression::Name("string".to_owned()),
            ]),
        );
        assert_eq!(consumed, "int|string".len());
        assert!(parse_type_expression_prefix("$x only prose").is_none());
    }

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
