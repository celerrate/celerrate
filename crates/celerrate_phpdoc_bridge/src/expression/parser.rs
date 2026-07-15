//! The recursive-descent grammar over the token stream. Every parse
//! function threads the shared depth guard: adversarial nesting
//! answers `None`, never a stack overflow. Grammar failures answer
//! `None` for the whole expression — loss is per construct (one tag
//! element), never per annotation.

use super::tokens::{Token, TokenKind};
use super::{
    CallableParameterExpression, ConditionalSubject, ShapeFieldExpression, ShapeKeyExpression,
    TypeExpression, UnsealedTail,
};

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

    pub(crate) fn checkpoint(&self) -> usize {
        self.position
    }

    pub(crate) fn rewind(&mut self, checkpoint: usize) {
        self.position = checkpoint;
    }

    pub(crate) fn peek_token(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }
}

pub(crate) fn parse_type(parser: &mut Parser<'_>, depth: u32) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    // Conditional lookahead: a bare name or `$variable` followed by
    // `is` opens a conditional type; everything else is a union.
    let subject = match (parser.peek(), parser.peek_at(1)) {
        (Some(TokenKind::Variable(name)), Some(TokenKind::Name(keyword))) if keyword == "is" => {
            Some(ConditionalSubject::Parameter(name.clone()))
        }
        (Some(TokenKind::Name(name)), Some(TokenKind::Name(keyword))) if keyword == "is" => {
            Some(ConditionalSubject::Template(name.clone()))
        }
        _ => None,
    };
    if let Some(subject) = subject {
        // Prose can begin with `is` too (`@return Foo is the widget`):
        // a failed conditional rewinds and the plain union stands, so
        // the annotation survives with the prose as remainder.
        let checkpoint = parser.checkpoint();
        if let Some(conditional) = parse_conditional(parser, depth, subject) {
            return Some(conditional);
        }
        parser.rewind(checkpoint);
    }
    parse_union(parser, depth)
}

fn parse_conditional(
    parser: &mut Parser<'_>,
    depth: u32,
    subject: ConditionalSubject,
) -> Option<TypeExpression> {
    parser.advance(); // the subject
    parser.advance(); // `is`
    let negated = matches!(parser.peek(), Some(TokenKind::Name(keyword)) if keyword == "not");
    if negated {
        parser.advance();
    }
    let target = parse_union(parser, depth + 1)?;
    if !parser.eat(&TokenKind::Question) {
        return None;
    }
    let then_branch = parse_type(parser, depth + 1)?;
    if !parser.eat(&TokenKind::Colon) {
        return None;
    }
    let otherwise_branch = parse_type(parser, depth + 1)?;
    Some(TypeExpression::Conditional {
        subject,
        negated,
        target: Box::new(target),
        then_branch: Box::new(then_branch),
        otherwise_branch: Box::new(otherwise_branch),
    })
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
    loop {
        if parser.peek() != Some(&TokenKind::Ampersand) {
            break;
        }
        // `&` doubles as the by-reference marker in callable
        // signatures (`callable(string&$out)`): it continues an
        // intersection only when a type can follow it.
        if matches!(
            parser.peek_at(1),
            Some(
                TokenKind::Variable(_)
                    | TokenKind::Ellipsis
                    | TokenKind::Comma
                    | TokenKind::CloseParenthesis
                    | TokenKind::Equals
            ) | None
        ) {
            break;
        }
        parser.advance();
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
        if parser.peek() == Some(&TokenKind::OpenBracket) {
            parser.advance();
            let offset = parse_type(parser, depth + 1)?;
            if !parser.eat(&TokenKind::CloseBracket) {
                return None;
            }
            expression = TypeExpression::Offset {
                base: Box::new(expression),
                offset: Box::new(offset),
            };
            continue;
        }
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
        TokenKind::Variable(name) if name == "this" => {
            parser.advance();
            Some(TypeExpression::This)
        }
        TokenKind::Name(name) => {
            let name = name.clone();
            parser.advance();
            if parser.eat(&TokenKind::DoubleColon) {
                let constant = parse_constant_name(parser)?;
                return Some(TypeExpression::ConstFetch {
                    class: name,
                    constant,
                });
            }
            if parser.peek() == Some(&TokenKind::OpenAngle) {
                if is_callable_base(&name) {
                    // `Closure<T of Foo>(T): T` — try the callable
                    // template list; rewind to a generic on failure.
                    let checkpoint = parser.checkpoint();
                    parser.advance();
                    if let Some(templates) = parse_callable_templates(parser, depth + 1)
                        && parser.peek() == Some(&TokenKind::OpenParenthesis)
                    {
                        return parse_callable_signature(parser, depth, name, templates);
                    }
                    parser.rewind(checkpoint);
                }
                parser.advance();
                let arguments = parse_generic_arguments(parser, depth + 1)?;
                return Some(TypeExpression::Generic {
                    base: name,
                    arguments,
                });
            }
            if is_callable_base(&name) && parser.peek() == Some(&TokenKind::OpenParenthesis) {
                return parse_callable_signature(parser, depth, name, Vec::new());
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
            Some(TypeExpression::Name(name))
        }
        _ => None,
    }
}

/// The `<...>` argument list of a name-headed generic. Call-site
/// variance keywords (`covariant`, `contravariant`) are consumed and
/// dropped — the ignored-variance posture, documented in the ledger.
/// A bare `*` argument (the bivariant wildcard: "unknown, don't care")
/// carries no type of its own and lowers to `mixed`, the same posture.
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
        if parser.peek() == Some(&TokenKind::Asterisk) {
            parser.advance();
            arguments.push(TypeExpression::Name("mixed".to_owned()));
        } else {
            arguments.push(parse_type(parser, depth + 1)?);
        }
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

/// The bases the reference accepts a `(signature)` on. The purity
/// prefixes lower with their purity dropped (documented).
fn is_callable_base(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().trim_start_matches('\\'),
        "callable" | "closure" | "pure-callable" | "pure-closure"
    )
}

/// `Foo::BAR`, `Foo::*`, `Foo::BAR_*`: the constant is the adjacent
/// run of name and `*` tokens after the `::` (adjacency by byte
/// offset — whitespace breaks the run).
fn parse_constant_name(parser: &mut Parser<'_>) -> Option<String> {
    let mut constant = String::new();
    let mut previous_end: Option<usize> = None;
    while let Some(token) = parser.peek_token() {
        if previous_end.is_some_and(|end| token.start != end) {
            break;
        }
        match &token.kind {
            TokenKind::Name(part) => constant.push_str(part),
            TokenKind::Asterisk => constant.push('*'),
            _ => break,
        }
        previous_end = Some(token.end);
        parser.advance();
    }
    if constant.is_empty() {
        None
    } else {
        Some(constant)
    }
}

/// The `<T, U of Bound>` list of a callable. Bounds are parsed and
/// dropped (decision 12); the caller has already consumed the `<`.
fn parse_callable_templates(parser: &mut Parser<'_>, depth: u32) -> Option<Vec<String>> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    let mut templates = Vec::new();
    loop {
        let Some(TokenKind::Name(name)) = parser.peek() else {
            return None;
        };
        templates.push(name.clone());
        parser.advance();
        if let Some(TokenKind::Name(keyword)) = parser.peek()
            && (keyword == "of" || keyword == "as")
        {
            parser.advance();
            let _bound = parse_type(parser, depth + 1)?;
        }
        if parser.eat(&TokenKind::Comma) {
            if parser.eat(&TokenKind::CloseAngle) {
                return Some(templates);
            }
            continue;
        }
        if parser.eat(&TokenKind::CloseAngle) {
            return Some(templates);
        }
        return None;
    }
}

/// `(type [&] [...] [$name] [=], ...) : return`. Parameter names are
/// parsed and dropped (the lattice's `CallableParameter` carries
/// none); the return type is required, per the reference.
fn parse_callable_signature(
    parser: &mut Parser<'_>,
    depth: u32,
    base: String,
    templates: Vec<String>,
) -> Option<TypeExpression> {
    if depth >= MAXIMUM_DEPTH {
        return None;
    }
    if !parser.eat(&TokenKind::OpenParenthesis) {
        return None;
    }
    let mut parameters = Vec::new();
    if !parser.eat(&TokenKind::CloseParenthesis) {
        loop {
            let parameter_type = parse_type(parser, depth + 1)?;
            let by_reference = parser.eat(&TokenKind::Ampersand);
            let variadic = parser.eat(&TokenKind::Ellipsis);
            if matches!(parser.peek(), Some(TokenKind::Variable(_))) {
                parser.advance();
            }
            let optional = parser.eat(&TokenKind::Equals);
            parameters.push(CallableParameterExpression {
                parameter_type,
                by_reference,
                variadic,
                optional,
            });
            if parser.eat(&TokenKind::Comma) {
                // A trailing comma before the closing parenthesis is
                // tolerated, same as the shape and generic argument
                // lists (`parse_shape_body`, `parse_generic_arguments`).
                if parser.eat(&TokenKind::CloseParenthesis) {
                    break;
                }
                continue;
            }
            if parser.eat(&TokenKind::CloseParenthesis) {
                break;
            }
            return None;
        }
    }
    if !parser.eat(&TokenKind::Colon) {
        return None;
    }
    let return_type = parse_suffixed(parser, depth + 1)?;
    Some(TypeExpression::Callable {
        base,
        templates,
        parameters,
        return_type: Box::new(return_type),
    })
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
