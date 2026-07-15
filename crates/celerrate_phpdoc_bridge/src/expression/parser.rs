//! The recursive-descent grammar over the token stream. Every parse
//! function threads the shared depth guard: adversarial nesting
//! answers `None`, never a stack overflow. Grammar failures answer
//! `None` for the whole expression — loss is per construct (one tag
//! element), never per annotation.

use super::tokens::{Token, TokenKind};
use super::{ShapeFieldExpression, ShapeKeyExpression, TypeExpression, UnsealedTail};

/// Nesting is refused past this depth: adversarial input (`(((((...`)
/// must not overflow the stack.
pub(crate) const MAXIMUM_DEPTH: u32 = 64;

pub(crate) struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    pub(crate) fn peek(&self) -> Option<&'a TokenKind> {
        self.tokens.get(self.position).map(|token| &token.kind)
    }

    pub(crate) fn peek_at(&self, offset: usize) -> Option<&'a TokenKind> {
        self.tokens
            .get(self.position + offset)
            .map(|token| &token.kind)
    }

    pub(crate) fn advance(&mut self) -> Option<&'a TokenKind> {
        let token = self.tokens.get(self.position)?;
        self.position += 1;
        Some(&token.kind)
    }

    /// Consumes the next token when it equals `expected` (payload-free
    /// punctuation kinds).
    pub(crate) fn eat(&mut self, expected: &TokenKind) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    /// The byte offset just past the last consumed token — the
    /// consumed-prefix length the tag layer slices with.
    pub(crate) fn consumed_end(&self) -> Option<usize> {
        self.position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map(|token| token.end)
    }
}

pub(crate) fn parse_type(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    // Task 5 adds the conditional tail (`is`) here.
    parse_union(parser, depth)
}

fn parse_union(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut members = vec![parse_intersection(parser, depth + 1)?];
    while parser.eat(&TokenKind::Pipe) {
        members.push(parse_intersection(parser, depth + 1)?);
    }
    if members.len() == 1 {
        members.into_iter().next()
    } else {
        Some(TypeExpression::Union(members))
    }
}

fn parse_intersection(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut members = vec![parse_suffixed(parser, depth + 1)?];
    while parser.eat(&TokenKind::Ampersand) {
        members.push(parse_suffixed(parser, depth + 1)?);
    }
    if members.len() == 1 {
        members.into_iter().next()
    } else {
        Some(TypeExpression::Intersection(members))
    }
}

fn parse_suffixed(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut expression = parse_atom(parser, depth + 1)?;
    loop {
        if parser.peek() == Some(&TokenKind::OpenBracket)
            && parser.peek_at(1) == Some(&TokenKind::CloseBracket)
        {
            parser.advance();
            parser.advance();
            expression = TypeExpression::ArrayOf(Box::new(expression));
            continue;
        }
        // Task 5 adds offset access (`[` type `]`) here.
        break;
    }
    Some(expression)
}

fn parse_atom(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    match parser.peek()? {
        TokenKind::Question => {
            parser.advance();
            // The nullable marker binds to the atom; array suffixes
            // wrap outside (`?int[]` is `(?int)[]`) — decision 4.
            let inner = parse_atom(parser, depth + 1)?;
            Some(TypeExpression::Nullable(Box::new(inner)))
        }
        TokenKind::OpenParenthesis => {
            parser.advance();
            let inner = parse_type(parser, depth + 1)?;
            if parser.eat(&TokenKind::CloseParenthesis) {
                Some(inner)
            } else {
                None
            }
        }
        TokenKind::Integer(value) => {
            let value = *value;
            parser.advance();
            Some(TypeExpression::IntLiteral(value))
        }
        TokenKind::Float(text) => {
            let text = text.clone();
            parser.advance();
            Some(TypeExpression::FloatLiteral(text))
        }
        TokenKind::StringLiteral(value) => {
            let value = value.clone();
            parser.advance();
            Some(TypeExpression::StringLiteral(value))
        }
        TokenKind::Name(name) => {
            let name = name.clone();
            parser.advance();
            if parser.eat(&TokenKind::OpenAngle) {
                let arguments = parse_generic_arguments(parser, depth + 1)?;
                return Some(TypeExpression::Generic {
                    base: name,
                    arguments,
                });
            }
            if is_shape_base(&name) && parser.peek() == Some(&TokenKind::OpenBrace) {
                parser.advance();
                let (fields, unsealed) = parse_shape_body(parser, depth + 1)?;
                return Some(TypeExpression::Shape {
                    base: name,
                    fields,
                    unsealed,
                });
            }
            // Task 5 extends name-headed constructs here (callables,
            // const fetches).
            Some(TypeExpression::Name(name))
        }
        _ => None,
    }
}

/// The `<...>` argument list of a name-headed generic. Call-site
/// variance keywords (`covariant`, `contravariant`) are consumed and
/// dropped — the ignored-variance posture, documented in the ledger.
/// A trailing comma is tolerated; an empty list is refused.
fn parse_generic_arguments(parser: &mut Parser<'_>, depth: u32) -> Option<Vec<TypeExpression>> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut arguments = Vec::new();
    loop {
        if let Some(TokenKind::Name(keyword)) = parser.peek()
            && (keyword == "covariant" || keyword == "contravariant")
            && !matches!(
                parser.peek_at(1),
                Some(TokenKind::CloseAngle | TokenKind::Comma) | None
            )
        {
            parser.advance();
        }
        arguments.push(parse_type(parser, depth + 1)?);
        if parser.eat(&TokenKind::Comma) {
            if parser.eat(&TokenKind::CloseAngle) {
                return Some(arguments);
            }
            continue;
        }
        if parser.eat(&TokenKind::CloseAngle) {
            return Some(arguments);
        }
        return None;
    }
}

/// The bases the reference accepts a `{...}` body on. Everything else
/// keeps its brace unconsumed: the name ends the prefix.
fn is_shape_base(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "array" | "non-empty-array" | "list" | "non-empty-list" | "object"
    )
}

/// The `{...}` body: fields, an optional unsealed tail (`...`,
/// `...<V>`, `...<K, V>`, always last), trailing commas tolerated.
fn parse_shape_body(
    parser: &mut Parser<'_>,
    depth: u32,
) -> Option<(Vec<ShapeFieldExpression>, Option<UnsealedTail>)> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut fields = Vec::new();
    if parser.eat(&TokenKind::CloseBrace) {
        return Some((fields, None));
    }
    loop {
        if parser.eat(&TokenKind::Ellipsis) {
            let tail = parse_unsealed_tail(parser, depth + 1)?;
            let _ = parser.eat(&TokenKind::Comma);
            if parser.eat(&TokenKind::CloseBrace) {
                return Some((fields, Some(tail)));
            }
            return None;
        }
        fields.push(parse_shape_field(parser, depth + 1)?);
        if parser.eat(&TokenKind::Comma) {
            if parser.eat(&TokenKind::CloseBrace) {
                return Some((fields, None));
            }
            continue;
        }
        if parser.eat(&TokenKind::CloseBrace) {
            return Some((fields, None));
        }
        return None;
    }
}

fn parse_unsealed_tail(parser: &mut Parser<'_>, depth: u32) -> Option<UnsealedTail> {
    if !parser.eat(&TokenKind::OpenAngle) {
        return Some(UnsealedTail {
            key: None,
            value: None,
        });
    }
    let first = parse_type(parser, depth + 1)?;
    if parser.eat(&TokenKind::Comma) {
        let second = parse_type(parser, depth + 1)?;
        if parser.eat(&TokenKind::CloseAngle) {
            return Some(UnsealedTail {
                key: Some(Box::new(first)),
                value: Some(Box::new(second)),
            });
        }
        return None;
    }
    if parser.eat(&TokenKind::CloseAngle) {
        return Some(UnsealedTail {
            key: None,
            value: Some(Box::new(first)),
        });
    }
    None
}

/// One field: `key ?': type`, `key: type`, or a keyless tuple entry.
/// Keys are identifiers, string literals, or integer literals — the
/// two-token lookahead (`key ':'` / `key '?' ':'`) is what separates
/// a key from a keyless field whose type happens to be a name.
fn parse_shape_field(parser: &mut Parser<'_>, depth: u32) -> Option<ShapeFieldExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let key = match parser.peek() {
        Some(TokenKind::Name(name)) => Some(ShapeKeyExpression::Identifier(name.clone())),
        Some(TokenKind::StringLiteral(value)) => Some(ShapeKeyExpression::String(value.clone())),
        Some(TokenKind::Integer(value)) => Some(ShapeKeyExpression::Integer(*value)),
        _ => None,
    };
    let keyed = key.is_some()
        && matches!(
            (parser.peek_at(1), parser.peek_at(2)),
            (Some(TokenKind::Colon), _) | (Some(TokenKind::Question), Some(TokenKind::Colon))
        );
    if keyed {
        parser.advance();
        let optional = parser.eat(&TokenKind::Question);
        if !parser.eat(&TokenKind::Colon) {
            return None;
        }
        let value = parse_type(parser, depth + 1)?;
        return Some(ShapeFieldExpression {
            key,
            optional,
            value,
        });
    }
    let value = parse_type(parser, depth + 1)?;
    Some(ShapeFieldExpression {
        key: None,
        optional: false,
        value,
    })
}
