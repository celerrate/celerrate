//! The recursive-descent grammar over the token stream. Every parse
//! function threads the shared depth guard: adversarial nesting
//! answers `None`, never a stack overflow. Grammar failures answer
//! `None` for the whole expression — loss is per construct (one tag
//! element), never per annotation.

use super::TypeExpression;
use super::tokens::{Token, TokenKind};

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
        TokenKind::Name(name) => {
            let name = name.clone();
            parser.advance();
            // Tasks 3-5 extend name-headed constructs here (generics,
            // shapes, callables, const fetches).
            Some(TypeExpression::Name(name))
        }
        _ => None,
    }
}
