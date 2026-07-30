//! Lowering the Celerrate norm's written form into lattice types.
//! First consumer: the stub
//! refinements overlay. Internal by design: the
//! norm is not a plugin notation, so this module is `pub(crate)`
//! and never crosses the facade. Tolerant: anything outside the v0
//! subset answers `None`, never a panic. The subset
//! boundary is exact and tested, not merely aspirational: forms that
//! once parsed as sound-but-undocumented over-approximations (bare
//! `array`/`list`/`iterable`/`non-empty-array`/`non-empty-list`, bare
//! `callable`, the empty shape `{}`, quoted shape keys, hyphenated
//! class names, and stacked `??T`) now answer `None`
//! (`forms_outside_the_documented_subset_are_rejected`, issue #48); the
//! three documented v0 conveniences (`array-key`, the single-argument
//! `array<V>`/`iterable<V>` sugars, and single `?T`) still lower
//! (`the_documented_conveniences_lower`).
//!
//! Recorded debt (owner: the norm v0 subset). Conditional types
//! (`(T is int ? A :
//! B)`) are excluded from v0 and answer `None`
//! (`everything_outside_the_subset_answers_none_never_a_panic`
//! pins the exact text below): the lattice has no conditional-type
//! constructor to lower into, and no refinement has needed one to
//! date. Refinements are version-agnostic by design:
//! `NormScope` carries no PHP-version parameter, so a curated
//! signature cannot express a per-version delta the way the base
//! phpstorm-stubs surface (outside this module) does; revisit only if
//! a curated signature ever needs one.

use crate::representation::{CallableParameter, ShapeField, ShapeKey, TypeId};

/// A template declared by the refinement entry under lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormTemplate<'db> {
    pub name: String,
    pub bound: Option<TypeId<'db>>,
}

/// The lowering context: the scope key template types intern under,
/// and the templates in scope (keywords shadow them).
pub(crate) struct NormScope<'db, 'a> {
    pub key: &'a str,
    pub templates: &'a [NormTemplate<'db>],
}

/// Lowers one norm type expression. `None` on anything outside the
/// v0 subset, tolerant of arbitrary bytes. The subset
/// boundary is tested, not just documented (issue #48): see
/// `forms_outside_the_documented_subset_are_rejected` and
/// `the_documented_conveniences_lower`.
pub(crate) fn lower_norm_text<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    text: &str,
) -> Option<TypeId<'db>> {
    let tokens = lex(text)?;
    let mut cursor = Cursor {
        tokens: &tokens,
        position: 0,
        depth: 0,
    };
    let lowered = union_type(db, scope, &mut cursor)?;
    cursor.at_end().then_some(lowered)
}

/// Cap on `atom_type` recursion depth (`union_type` -> `intersection_type`
/// -> `atom_type`, plus `?T`'s direct self-recursion, both cycle back
/// through this function). No legitimate norm text nests anywhere close
/// to this: phpstorm-stubs refinements are a handful of levels deep at
/// most. 256 is comfortably past any real input while staying far below
/// the stack a single recursive frame here could exhaust (each cycle
/// pushes only a few hundred bytes), so hostile input such as
/// `"(".repeat(100_000)` answers `None` instead of overflowing the stack.
const MAX_ATOM_NESTING_DEPTH: usize = 256;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    Integer(i64),
    Float(f64),
    Text(String),
    Question,
    Pipe,
    Ampersand,
    Comma,
    Colon,
    DoubleColon,
    LessThan,
    GreaterThan,
    OpenParenthesis,
    CloseParenthesis,
    OpenBrace,
    CloseBrace,
    Equals,
    Ellipsis,
    DotDot,
}

fn lex(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(&character) = characters.peek() {
        match character {
            character if character.is_whitespace() => {
                characters.next();
            }
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
            ',' => {
                characters.next();
                tokens.push(Token::Comma);
            }
            '<' => {
                characters.next();
                tokens.push(Token::LessThan);
            }
            '>' => {
                characters.next();
                tokens.push(Token::GreaterThan);
            }
            '(' => {
                characters.next();
                tokens.push(Token::OpenParenthesis);
            }
            ')' => {
                characters.next();
                tokens.push(Token::CloseParenthesis);
            }
            '{' => {
                characters.next();
                tokens.push(Token::OpenBrace);
            }
            '}' => {
                characters.next();
                tokens.push(Token::CloseBrace);
            }
            '=' => {
                characters.next();
                tokens.push(Token::Equals);
            }
            ':' => {
                characters.next();
                if characters.peek() == Some(&':') {
                    characters.next();
                    tokens.push(Token::DoubleColon);
                } else {
                    tokens.push(Token::Colon);
                }
            }
            '.' => {
                characters.next();
                match characters.peek() {
                    Some('.') => {
                        characters.next();
                        // Three dots are the variadic marker, two the
                        // range separator.
                        if characters.peek() == Some(&'.') {
                            characters.next();
                            tokens.push(Token::Ellipsis);
                        } else {
                            tokens.push(Token::DotDot);
                        }
                    }
                    _ => return None,
                }
            }
            '\'' => {
                characters.next();
                let mut value = String::new();
                loop {
                    match characters.next() {
                        Some('\'') => break,
                        Some(next) => value.push(next),
                        None => return None,
                    }
                }
                tokens.push(Token::Text(value));
            }
            '-' | '0'..='9' => tokens.push(lex_number(&mut characters)?),
            character if is_name_start(character) => {
                tokens.push(Token::Name(lex_name(&mut characters)));
            }
            _ => return None,
        }
    }
    Some(tokens)
}

fn is_name_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_' || character == '\\'
}

fn is_name_continuation(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '\\'
}

/// One (possibly qualified) name. A hyphen continues the name only
/// when a letter follows: `non-empty-string` and `key-of` lex whole,
/// while `int<1..-5>` leaves the minus to the number lexer.
fn lex_name(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut name = String::new();
    while let Some(&character) = characters.peek() {
        if is_name_continuation(character) {
            name.push(character);
            characters.next();
        } else if character == '-' {
            let mut lookahead = characters.clone();
            lookahead.next();
            match lookahead.peek() {
                Some(next) if next.is_ascii_alphabetic() => {
                    name.push('-');
                    characters.next();
                }
                _ => break,
            }
        } else {
            break;
        }
    }
    name
}

/// An integer or float literal, optionally negative. Digits followed
/// by `..` stay an integer (the range separator is not a fraction).
fn lex_number(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<Token> {
    let mut digits = String::new();
    if characters.peek() == Some(&'-') {
        digits.push('-');
        characters.next();
    }
    while let Some(&character) = characters.peek() {
        if character.is_ascii_digit() {
            digits.push(character);
            characters.next();
        } else {
            break;
        }
    }
    if digits.is_empty() || digits == "-" {
        return None;
    }
    if characters.peek() == Some(&'.') {
        let mut lookahead = characters.clone();
        lookahead.next();
        if lookahead
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            digits.push('.');
            characters.next();
            while let Some(&character) = characters.peek() {
                if character.is_ascii_digit() {
                    digits.push(character);
                    characters.next();
                } else {
                    break;
                }
            }
            return digits.parse().ok().map(Token::Float);
        }
    }
    digits.parse().ok().map(Token::Integer)
}

struct Cursor<'a> {
    tokens: &'a [Token],
    position: usize,
    /// Current `atom_type` call-stack depth. Incremented on entry and
    /// decremented on exit (see `atom_type`), so it tracks live nesting
    /// rather than the total number of atoms parsed.
    depth: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.position)?;
        self.position += 1;
        Some(token)
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn at_end(&self) -> bool {
        self.position == self.tokens.len()
    }
}

fn union_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    let mut constituents = vec![intersection_type(db, scope, cursor)?];
    while cursor.eat(&Token::Pipe) {
        constituents.push(intersection_type(db, scope, cursor)?);
    }
    Some(match constituents.as_slice() {
        [single] => *single,
        _ => TypeId::union(db, constituents),
    })
}

fn intersection_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    let mut intersectands = vec![atom_type(db, scope, cursor)?];
    while cursor.eat(&Token::Ampersand) {
        intersectands.push(atom_type(db, scope, cursor)?);
    }
    Some(match intersectands.as_slice() {
        [single] => *single,
        _ => TypeId::intersection(db, intersectands),
    })
}

/// Guards `atom_type_body`'s recursion depth (the tolerant-input
/// constraint: hostile input answers `None`, never crashes). See
/// `MAX_ATOM_NESTING_DEPTH`.
fn atom_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    cursor.depth += 1;
    let result = if cursor.depth > MAX_ATOM_NESTING_DEPTH {
        None
    } else {
        atom_type_body(db, scope, cursor)
    };
    cursor.depth -= 1;
    result
}

fn atom_type_body<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    match cursor.advance()? {
        // `?T` binds tighter than `|` and `&`. Stacked
        // nullable (`??T`) is outside the v0 subset (issue #48: one
        // spelling per constructor): peek for a second
        // `?` and reject rather than silently double-wrapping.
        Token::Question => {
            if cursor.peek() == Some(&Token::Question) {
                return None;
            }
            let inner = atom_type(db, scope, cursor)?;
            Some(TypeId::union(db, [inner, TypeId::null(db)]))
        }
        Token::OpenParenthesis => {
            let inner = union_type(db, scope, cursor)?;
            cursor.eat(&Token::CloseParenthesis).then_some(inner)
        }
        Token::OpenBrace => shape_type(db, scope, cursor),
        Token::Integer(value) => Some(TypeId::int_literal(db, *value)),
        Token::Float(value) => Some(TypeId::float_literal(db, *value)),
        Token::Text(value) => Some(TypeId::string_literal(db, value)),
        Token::Name(name) => named_type(db, scope, cursor, name),
        _ => None,
    }
}

fn named_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
    name: &str,
) -> Option<TypeId<'db>> {
    // Keywords first (they shadow templates and class names).
    match name {
        "mixed" => return Some(TypeId::mixed(db)),
        "never" => return Some(TypeId::never(db)),
        "void" => return Some(TypeId::void(db)),
        "null" => return Some(TypeId::null(db)),
        "object" => return Some(TypeId::object(db)),
        "resource" => return Some(TypeId::resource(db)),
        "bool" => return Some(TypeId::bool(db)),
        "true" => return Some(TypeId::bool_literal(db, true)),
        "false" => return Some(TypeId::bool_literal(db, false)),
        "float" => return Some(TypeId::float(db)),
        "string" => return Some(TypeId::string(db)),
        "non-empty-string" => return Some(TypeId::non_empty_string(db)),
        "numeric-string" => return Some(TypeId::numeric_string(db)),
        "literal-string" => return Some(TypeId::literal_string_type(db)),
        "array-key" => {
            return Some(TypeId::union(db, [TypeId::int(db), TypeId::string(db)]));
        }
        "static" => return Some(TypeId::static_placeholder(db)),
        "self" => return Some(TypeId::self_placeholder(db)),
        "parent" => return Some(TypeId::parent_placeholder(db)),
        "int" => return int_type(db, cursor),
        "array" => return array_type(db, scope, cursor, false),
        "non-empty-array" => return array_type(db, scope, cursor, true),
        "list" => return list_type(db, scope, cursor, false),
        "non-empty-list" => return list_type(db, scope, cursor, true),
        "iterable" => return iterable_type(db, scope, cursor),
        "class-string" => return class_string_type(db, scope, cursor),
        "key-of" => return projection_type(db, scope, cursor, TypeId::key_of),
        "value-of" => return projection_type(db, scope, cursor, TypeId::value_of),
        "callable" => return callable_type(db, scope, cursor),
        _ => {}
    }
    // A hyphenated name reaching here is none of the known hyphenated
    // keywords matched above: hyphens outside that closed keyword set
    // are outside the v0 subset (issue #48). The lexer still lexes
    // `Foo-Bar` as one name (`lex_name`'s job, unchanged); the
    // constraint lands here, on lowering.
    if name.contains('-') {
        return None;
    }
    // `Enum::Case` before template and class references.
    if cursor.eat(&Token::DoubleColon) {
        let Some(Token::Name(case)) = cursor.advance() else {
            return None;
        };
        return Some(TypeId::enum_case(db, name, case));
    }
    // Templates in scope, then class references.
    if let Some(template) = scope
        .templates
        .iter()
        .find(|template| template.name == name)
    {
        return Some(TypeId::template(
            db,
            scope.key,
            &template.name,
            template.bound.unwrap_or_else(|| TypeId::mixed(db)),
        ));
    }
    let arguments = match generic_arguments(db, scope, cursor) {
        Some(arguments) => arguments?,
        None => vec![],
    };
    Some(TypeId::class(db, name, arguments))
}

/// `Some(inner)` when a `<...>` argument list is present (inner is
/// `None` on a malformed list), `None` when absent.
#[allow(clippy::type_complexity)]
fn generic_arguments<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<Option<Vec<TypeId<'db>>>> {
    if !cursor.eat(&Token::LessThan) {
        return None;
    }
    let mut arguments = Vec::new();
    loop {
        match union_type(db, scope, cursor) {
            Some(argument) => arguments.push(argument),
            None => return Some(None),
        }
        if cursor.eat(&Token::GreaterThan) {
            return Some(Some(arguments));
        }
        if !cursor.eat(&Token::Comma) {
            return Some(None);
        }
    }
}

fn int_type<'db>(db: &'db dyn salsa::Database, cursor: &mut Cursor<'_>) -> Option<TypeId<'db>> {
    if !cursor.eat(&Token::LessThan) {
        return Some(TypeId::int(db));
    }
    let minimum = match cursor.peek() {
        Some(Token::Integer(value)) => {
            let value = *value;
            cursor.advance();
            Some(value)
        }
        _ => None,
    };
    if !cursor.eat(&Token::DotDot) {
        return None;
    }
    let maximum = match cursor.peek() {
        Some(Token::Integer(value)) => {
            let value = *value;
            cursor.advance();
            Some(value)
        }
        _ => None,
    };
    if minimum.is_none() && maximum.is_none() {
        return None;
    }
    // An inverted range (`int<5..1>`) is outside the v0 subset: hand
    // it to `TypeId::int_range` and it canonicalizes to `never`, the
    // most dangerous wrong answer this module can give (the
    // tolerant-input constraint forbids fabricating `never`). Reject it
    // here instead.
    if let (Some(low), Some(high)) = (minimum, maximum)
        && low > high
    {
        return None;
    }
    cursor
        .eat(&Token::GreaterThan)
        .then(|| TypeId::int_range(db, minimum, maximum))
}

fn array_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
    non_empty: bool,
) -> Option<TypeId<'db>> {
    // A bare `array`/`non-empty-array` (no `<...>` at all) is outside
    // the v0 subset: only the single-argument sugar is documented
    // (issue #48). `generic_arguments` answers `None` for "absent",
    // which the first `?` turns into this function's own `None`.
    let array_key = TypeId::union(db, [TypeId::int(db), TypeId::string(db)]);
    let (key, value) = match generic_arguments(db, scope, cursor)??.as_slice() {
        [value] => (array_key, *value),
        [key, value] => (*key, *value),
        _ => return None,
    };
    Some(if non_empty {
        TypeId::non_empty_array(db, key, value)
    } else {
        TypeId::array(db, key, value)
    })
}

fn list_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
    non_empty: bool,
) -> Option<TypeId<'db>> {
    // Same v0 boundary as `array_type`: a bare `list`/`non-empty-list`
    // answers `None` (issue #48).
    let value = match generic_arguments(db, scope, cursor)??.as_slice() {
        [value] => *value,
        _ => return None,
    };
    Some(if non_empty {
        TypeId::non_empty_list(db, value)
    } else {
        TypeId::list(db, value)
    })
}

fn iterable_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    // Same v0 boundary as `array_type`: a bare `iterable` answers
    // `None` (issue #48). The single-argument sugar stays documented.
    let (key, value) = match generic_arguments(db, scope, cursor)??.as_slice() {
        // Iterable keys are unconstrained: the array-key default
        // is only correct for arrays.
        [value] => (TypeId::mixed(db), *value),
        [key, value] => (*key, *value),
        _ => return None,
    };
    Some(TypeId::iterable(db, key, value))
}

fn class_string_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    match generic_arguments(db, scope, cursor) {
        None => Some(TypeId::class_string(db, None)),
        Some(arguments) => match arguments?.as_slice() {
            [argument] => Some(TypeId::class_string(db, Some(*argument))),
            _ => None,
        },
    }
}

fn projection_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
    construct: fn(&'db dyn salsa::Database, TypeId<'db>) -> TypeId<'db>,
) -> Option<TypeId<'db>> {
    match generic_arguments(db, scope, cursor)? {
        Some(arguments) => match arguments.as_slice() {
            [subject] => Some(construct(db, *subject)),
            _ => None,
        },
        None => None,
    }
}

fn callable_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    if !cursor.eat(&Token::OpenParenthesis) {
        // A bare `callable` (no parenthesized signature) is outside
        // the v0 subset (issue #48): the documented form always
        // carries a signature, even an empty one (`callable()`).
        return None;
    }
    let mut parameters = Vec::new();
    if !cursor.eat(&Token::CloseParenthesis) {
        loop {
            let parameter_type = union_type(db, scope, cursor)?;
            let optional = cursor.eat(&Token::Equals);
            let variadic = cursor.eat(&Token::Ellipsis);
            parameters.push(CallableParameter {
                parameter_type,
                optional,
                variadic,
                by_reference: false,
            });
            if cursor.eat(&Token::CloseParenthesis) {
                break;
            }
            if !cursor.eat(&Token::Comma) {
                return None;
            }
        }
    }
    let return_type = if cursor.eat(&Token::Colon) {
        union_type(db, scope, cursor)?
    } else {
        TypeId::mixed(db)
    };
    Some(TypeId::callable(db, parameters, return_type))
}

fn shape_type<'db>(
    db: &'db dyn salsa::Database,
    scope: &NormScope<'db, '_>,
    cursor: &mut Cursor<'_>,
) -> Option<TypeId<'db>> {
    // An empty shape (`{}`) is outside the v0 subset (issue #48): a
    // shape's whole point is to name its fields, so the empty-shape
    // spelling is left undocumented. Falling through to the loop below
    // lets the `_ => return None` arm reject it uniformly: the first
    // `advance` sees the closing brace, matches no key arm.
    let mut fields = Vec::new();
    loop {
        let key = match cursor.advance()? {
            Token::Name(name) => ShapeKey::String(name.clone()),
            Token::Integer(value) => ShapeKey::Integer(*value),
            // Quoted keys (`{'a': int}`) are outside the v0 subset
            // (issue #48): the documented grammar spells a shape key
            // as a bare name or integer, never a quoted string.
            _ => return None,
        };
        let optional = cursor.eat(&Token::Question);
        if !cursor.eat(&Token::Colon) {
            return None;
        }
        let value = union_type(db, scope, cursor)?;
        fields.push(ShapeField {
            key,
            optional,
            value,
        });
        if cursor.eat(&Token::CloseBrace) {
            return Some(TypeId::shape(db, fields));
        }
        if !cursor.eat(&Token::Comma) {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use super::{NormScope, NormTemplate, lower_norm_text};
    use crate::representation::TypeId;

    fn scope<'a>() -> NormScope<'static, 'a> {
        NormScope {
            key: "test_scope",
            templates: &[],
        }
    }

    fn lowered(db: &TestDatabase, text: &str) -> String {
        lower_norm_text(db, &scope(), text)
            .map(|type_id| type_id.display(db))
            .unwrap_or_else(|| "<none>".to_owned())
    }

    /// Mirrors `everything_outside_the_subset_answers_none_never_a_panic`:
    /// asserts `text` lowers to `None`.
    fn assert_lowers_to_none(text: &str) {
        let db = TestDatabase::default();
        assert!(
            lower_norm_text(&db, &scope(), text).is_none(),
            "expected None for {text:?}",
        );
    }

    /// The display-string assertion pattern used throughout this
    /// module (see `lowered` above): asserts `text` lowers to a type
    /// whose display rendering is `expected`.
    fn assert_lowers(text: &str, expected: &str) {
        let db = TestDatabase::default();
        assert_eq!(lowered(&db, text), expected, "for {text}");
    }

    #[test]
    fn keyword_atoms_lower_to_their_constructors() {
        let db = TestDatabase::default();
        for (text, expected) in [
            ("mixed", "mixed"),
            ("never", "never"),
            ("void", "void"),
            ("null", "null"),
            ("object", "object"),
            ("resource", "resource"),
            ("bool", "bool"),
            ("true", "true"),
            ("false", "false"),
            ("int", "int"),
            ("float", "float"),
            ("string", "string"),
            ("non-empty-string", "non-empty-string"),
            ("numeric-string", "numeric-string"),
            ("literal-string", "literal-string"),
        ] {
            assert_eq!(lowered(&db, text), expected, "for {text}");
        }
    }

    #[test]
    fn array_key_is_sugar_for_int_or_string() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "array-key").unwrap(),
            TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]),
        );
    }

    #[test]
    fn literals_lower() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "42").unwrap(),
            TypeId::int_literal(&db, 42),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "-7").unwrap(),
            TypeId::int_literal(&db, -7),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "'active'").unwrap(),
            TypeId::string_literal(&db, "active"),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "3.5").unwrap(),
            TypeId::float_literal(&db, 3.5),
        );
    }

    #[test]
    fn integer_ranges_use_the_dotdot_spelling() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "int<1..5>").unwrap(),
            TypeId::int_range(&db, Some(1), Some(5)),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "int<1..>").unwrap(),
            TypeId::int_range(&db, Some(1), None),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "int<..5>").unwrap(),
            TypeId::int_range(&db, None, Some(5)),
        );
        // The PHPStan `min`/`max` keywords do not exist in the norm.
        assert_eq!(lowered(&db, "int<1, max>"), "<none>");
    }

    #[test]
    fn arrays_lists_and_iterables_lower() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let string = TypeId::string(&db);
        assert_eq!(
            lower_norm_text(&db, &scope(), "array<int, string>").unwrap(),
            TypeId::array(&db, int, string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "array<string>").unwrap(),
            TypeId::array(&db, TypeId::union(&db, [int, string]), string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "non-empty-array<int, string>").unwrap(),
            TypeId::non_empty_array(&db, int, string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "list<string>").unwrap(),
            TypeId::list(&db, string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "non-empty-list<string>").unwrap(),
            TypeId::non_empty_list(&db, string),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "iterable<int, string>").unwrap(),
            TypeId::iterable(&db, int, string),
        );
        // Single-argument sugar: iterable keys are unconstrained.
        assert_eq!(
            lower_norm_text(&db, &scope(), "iterable<string>").unwrap(),
            TypeId::iterable(&db, TypeId::mixed(&db), string),
        );
    }

    #[test]
    fn shapes_drop_the_array_prefix_and_mark_optional_fields() {
        let db = TestDatabase::default();
        use crate::representation::{ShapeField, ShapeKey};
        assert_eq!(
            lower_norm_text(&db, &scope(), "{id: int, name?: string}").unwrap(),
            TypeId::shape(
                &db,
                vec![
                    ShapeField {
                        key: ShapeKey::String("id".to_owned()),
                        optional: false,
                        value: TypeId::int(&db),
                    },
                    ShapeField {
                        key: ShapeKey::String("name".to_owned()),
                        optional: true,
                        value: TypeId::string(&db),
                    },
                ],
            ),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "{0: string, 1?: string}").unwrap(),
            TypeId::shape(
                &db,
                vec![
                    ShapeField {
                        key: ShapeKey::Integer(0),
                        optional: false,
                        value: TypeId::string(&db),
                    },
                    ShapeField {
                        key: ShapeKey::Integer(1),
                        optional: true,
                        value: TypeId::string(&db),
                    },
                ],
            ),
        );
    }

    #[test]
    fn unions_intersections_and_nullable_compose() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "int|string").unwrap(),
            TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "Countable&Traversable").unwrap(),
            TypeId::intersection(
                &db,
                [
                    TypeId::class(&db, "countable", vec![]),
                    TypeId::class(&db, "traversable", vec![]),
                ],
            ),
        );
        // `?` binds tighter than `|` (norm open question 2, answered):
        // `?A|B` is `(A|null)|B`.
        assert_eq!(
            lower_norm_text(&db, &scope(), "?int|string").unwrap(),
            TypeId::union(
                &db,
                [TypeId::int(&db), TypeId::null(&db), TypeId::string(&db)],
            ),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "(A&B)|C").unwrap(),
            TypeId::union(
                &db,
                [
                    TypeId::intersection(
                        &db,
                        [
                            TypeId::class(&db, "a", vec![]),
                            TypeId::class(&db, "b", vec![]),
                        ],
                    ),
                    TypeId::class(&db, "c", vec![]),
                ],
            ),
        );
    }

    #[test]
    fn class_references_carry_generic_arguments() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "Collection<User>").unwrap(),
            TypeId::class(&db, "collection", vec![TypeId::class(&db, "user", vec![])],),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), r"Doctrine\Common\Collections\Collection").unwrap(),
            TypeId::class(&db, r"doctrine\common\collections\collection", vec![]),
        );
    }

    #[test]
    fn enum_cases_class_strings_and_projections_lower() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "Status::Active").unwrap(),
            TypeId::enum_case(&db, "status", "Active"),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "class-string").unwrap(),
            TypeId::class_string(&db, None),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "class-string<User>").unwrap(),
            TypeId::class_string(&db, Some(TypeId::class(&db, "user", vec![]))),
        );
        let subject = TypeId::class(&db, "config", vec![]);
        assert_eq!(
            lower_norm_text(&db, &scope(), "key-of<Config>").unwrap(),
            TypeId::key_of(&db, subject),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "value-of<Config>").unwrap(),
            TypeId::value_of(&db, subject),
        );
    }

    #[test]
    fn callables_lower_with_optional_and_variadic_markers() {
        let db = TestDatabase::default();
        use crate::representation::CallableParameter;
        assert_eq!(
            lower_norm_text(&db, &scope(), "callable(int, string=, bool...): void").unwrap(),
            TypeId::callable(
                &db,
                vec![
                    CallableParameter {
                        parameter_type: TypeId::int(&db),
                        optional: false,
                        variadic: false,
                        by_reference: false,
                    },
                    CallableParameter {
                        parameter_type: TypeId::string(&db),
                        optional: true,
                        variadic: false,
                        by_reference: false,
                    },
                    CallableParameter {
                        parameter_type: TypeId::bool(&db),
                        optional: false,
                        variadic: true,
                        by_reference: false,
                    },
                ],
                TypeId::void(&db),
            ),
        );
        // An omitted return is `mixed`.
        assert_eq!(
            lower_norm_text(&db, &scope(), "callable(int)").unwrap(),
            TypeId::callable(
                &db,
                vec![CallableParameter {
                    parameter_type: TypeId::int(&db),
                    optional: false,
                    variadic: false,
                    by_reference: false,
                }],
                TypeId::mixed(&db),
            ),
        );
    }

    #[test]
    fn templates_resolve_against_the_scope_keywords_first() {
        let db = TestDatabase::default();
        let templates = vec![
            NormTemplate {
                name: "TKey".to_owned(),
                bound: None,
            },
            NormTemplate {
                name: "TValue".to_owned(),
                bound: Some(TypeId::object(&db)),
            },
        ];
        let scope = NormScope {
            key: "array_keys",
            templates: &templates,
        };
        assert_eq!(
            lower_norm_text(&db, &scope, "list<TKey>").unwrap(),
            TypeId::list(
                &db,
                TypeId::template(&db, "array_keys", "TKey", TypeId::mixed(&db)),
            ),
        );
        assert_eq!(
            lower_norm_text(&db, &scope, "TValue").unwrap(),
            TypeId::template(&db, "array_keys", "TValue", TypeId::object(&db)),
        );
        // A keyword shadows a template of the same spelling: names are
        // matched keywords-first.
        let shadowing = vec![NormTemplate {
            name: "int".to_owned(),
            bound: None,
        }];
        let scope = NormScope {
            key: "x",
            templates: &shadowing,
        };
        assert_eq!(
            lower_norm_text(&db, &scope, "int").unwrap(),
            TypeId::int(&db),
        );
    }

    #[test]
    fn placeholders_lower() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), "static").unwrap(),
            TypeId::static_placeholder(&db),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "self").unwrap(),
            TypeId::self_placeholder(&db),
        );
        assert_eq!(
            lower_norm_text(&db, &scope(), "parent").unwrap(),
            TypeId::parent_placeholder(&db),
        );
    }

    #[test]
    fn whitespace_is_insignificant() {
        let db = TestDatabase::default();
        assert_eq!(
            lower_norm_text(&db, &scope(), " array< int , string > ").unwrap(),
            TypeId::array(&db, TypeId::int(&db), TypeId::string(&db)),
        );
    }

    #[test]
    fn everything_outside_the_subset_answers_none_never_a_panic() {
        let db = TestDatabase::default();
        for text in [
            "",
            "(T is int ? A : B)", // conditionals: excluded from v0
            "int|",
            "array<int,",
            "{id int}",
            "int<1..5", // unterminated
            "callable(int",
            "list<string> extra",
            "'unterminated",
            "T[]", // rule 6: the suffix form does not exist
            "?",
            "\u{0}\u{1}\u{2}",
            "int<<>>",
            "42.4.2",
            "int<5..1>",  // inverted range: low > high
            "int<1..-5>", // inverted range: low > high, negative bound
        ] {
            assert!(
                lower_norm_text(&db, &scope(), text).is_none(),
                "expected None for {text:?}",
            );
        }
    }

    #[test]
    fn forms_outside_the_documented_subset_are_rejected() {
        // Issue #48: each of these parsed to a sound over-approximation
        // with no test pinning it; the documented subset (one spelling
        // per constructor) does not
        // name them, and an undocumented accepted spelling is
        // compatibility debt in a grammar that intends to freeze.
        for rejected in [
            "array",
            "list",
            "iterable",
            "non-empty-array",
            "non-empty-list",
            "callable",
            "{}",
            "{'a': int}",
            "Foo-Bar",
            "??int",
        ] {
            assert_lowers_to_none(rejected);
        }
    }

    #[test]
    fn the_documented_conveniences_lower() {
        // The three documented v0 conveniences, positively pinned.
        assert_lowers("array-key", "int|string");
        assert_lowers("array<string>", "array<int|string, string>");
        assert_lowers(
            "iterable<string>",
            "array<mixed, string>|traversable<mixed, string>",
        );
    }

    #[test]
    fn deeply_nested_input_answers_none_instead_of_overflowing_the_stack() {
        let db = TestDatabase::default();
        // Comfortably past `MAX_ATOM_NESTING_DEPTH` (256). Each of these
        // alone crashes the process (stack overflow) without the depth
        // guard in `atom_type` — see the fix report for the observed
        // crash with the guard removed.
        let deeply_nested_parentheses = "(".repeat(100_000);
        let deeply_nested_nullable = "?".repeat(100_000);
        assert!(lower_norm_text(&db, &scope(), &deeply_nested_parentheses).is_none());
        assert!(lower_norm_text(&db, &scope(), &deeply_nested_nullable).is_none());
    }

    #[test]
    fn every_norm_alphabet_soup_is_parsed_or_rejected_without_panicking() {
        // A cheap fuzz floor, matching written.rs's
        // `every_ascii_soup_is_parsed_or_rejected_without_panicking`:
        // three-byte soups over an alphabet drawn from the norm's own
        // grammar (union, intersection, nullable, generics, ranges,
        // shapes, enum cases, strings, names).
        let db = TestDatabase::default();
        let alphabet = [
            b'A', b' ', b'?', b'|', b'&', b'(', b')', b'<', b'>', b'{', b'}', b':', b'.', b',',
            b'=', b'\'', b'1', b'_', b'-', b'\\',
        ];
        for &a in &alphabet {
            for &b in &alphabet {
                for &c in &alphabet {
                    let text: String = [a, b, c].iter().map(|&byte| byte as char).collect();
                    let _ = lower_norm_text(&db, &scope(), &text);
                }
            }
        }
    }
}
