//! Pure typing rules for expression atoms and operators: `TypeId` in,
//! `TypeId` out, no environment, no queries beyond the interner. The
//! rules are fold-free (no constant arithmetic — a folded value would
//! be a precision promise the typed check families
//! (`checks::arguments`, `checks::nullability`, `checks::members`)
//! would have to keep); the one exception is unary minus on an integer
//! literal, because negative literals feed `match` and `===` narrowing.
//! `mixed` is the answer to every form the table does not know:
//! silence, never a guess.

use celerrate_syntax::SyntaxKind;

use crate::representation::TypeId;
use crate::widening::join;

/// `int|float`, the numeric fallback of the arithmetic rules.
fn int_or_float<'db>(db: &'db dyn salsa::Database) -> TypeId<'db> {
    TypeId::union(db, [TypeId::int(db), TypeId::float(db)])
}

fn is_int<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.int_bounds(db).is_some()
}

fn is_float<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of == TypeId::float(db) || of.float_literal_value(db).is_some()
}

fn is_array<'db>(db: &'db dyn salsa::Database, of: TypeId<'db>) -> bool {
    of.array_value(db).is_some() || of.shape_fields(db).is_some()
}

/// An integer, float, or single-quoted string literal, as written
/// (the body IR's `Literal { text }` contract). An integer form that
/// overflows `i64` types as `float`, PHP's own overflow rule; an
/// unparseable form answers the general scalar.
pub(crate) fn literal_type<'db>(db: &'db dyn salsa::Database, text: &str) -> TypeId<'db> {
    if let Some(quoted) = text.strip_prefix('\'') {
        let inner = quoted.strip_suffix('\'').unwrap_or(quoted);
        let mut value = String::with_capacity(inner.len());
        let mut characters = inner.chars();
        while let Some(character) = characters.next() {
            if character == '\\' {
                match characters.next() {
                    Some('\'') => value.push('\''),
                    Some('\\') => value.push('\\'),
                    Some(other) => {
                        value.push('\\');
                        value.push(other);
                    }
                    None => value.push('\\'),
                }
            } else {
                value.push(character);
            }
        }
        return TypeId::string_literal(db, &value);
    }
    let digits: String = text.chars().filter(|&character| character != '_').collect();
    let float_like = digits.contains('.')
        || ((digits.contains('e') || digits.contains('E'))
            && !digits.starts_with("0x")
            && !digits.starts_with("0X"));
    if float_like {
        return match digits.parse::<f64>() {
            Ok(value) => TypeId::float_literal(db, value),
            Err(_) => TypeId::float(db),
        };
    }
    let parsed = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16)
    } else if let Some(binary) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        i64::from_str_radix(binary, 2)
    } else if let Some(octal) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        i64::from_str_radix(octal, 8)
    } else if digits.len() > 1 && digits.starts_with('0') {
        i64::from_str_radix(digits.get(1..).unwrap_or(""), 8)
    } else {
        digits.parse::<i64>()
    };
    match parsed {
        Ok(value) => TypeId::int_literal(db, value),
        // Overflow (PHP widens to float) and any unparseable residue.
        Err(_) if digits.chars().next().is_some_and(|c| c.is_ascii_digit()) => TypeId::float(db),
        Err(_) => TypeId::int(db),
    }
}

/// `true`, `false`, `null` parse as names (the body IR contract);
/// anything else is an ordinary constant fetch the caller types
/// `mixed`.
pub(crate) fn named_reference_type<'db>(
    db: &'db dyn salsa::Database,
    text: &str,
) -> Option<TypeId<'db>> {
    match text.to_ascii_lowercase().as_str() {
        "true" => Some(TypeId::bool_literal(db, true)),
        "false" => Some(TypeId::bool_literal(db, false)),
        "null" => Some(TypeId::null(db)),
        _ => None,
    }
}

pub(crate) fn cast_type<'db>(
    db: &'db dyn salsa::Database,
    operator: SyntaxKind,
    operand: TypeId<'db>,
) -> TypeId<'db> {
    match operator {
        SyntaxKind::IntCast => TypeId::int(db),
        SyntaxKind::BoolCast => TypeId::bool(db),
        SyntaxKind::FloatCast => TypeId::float(db),
        SyntaxKind::StringCast | SyntaxKind::BinaryCast => TypeId::string(db),
        SyntaxKind::ObjectCast => TypeId::object(db),
        SyntaxKind::ArrayCast => {
            if is_array(db, operand) {
                operand
            } else {
                TypeId::array(
                    db,
                    TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
                    TypeId::mixed(db),
                )
            }
        }
        _ => TypeId::mixed(db),
    }
}

pub(crate) fn unary_type<'db>(
    db: &'db dyn salsa::Database,
    operator: SyntaxKind,
    operand: TypeId<'db>,
) -> TypeId<'db> {
    match operator {
        SyntaxKind::Bang => TypeId::bool(db),
        SyntaxKind::Tilde => TypeId::int(db),
        SyntaxKind::Minus | SyntaxKind::Plus => {
            if operator == SyntaxKind::Minus
                && let Some(value) = operand.int_literal_value(db)
            {
                return TypeId::int_literal(db, value.saturating_neg());
            }
            numeric_preserving(db, operand)
        }
        // `@$x` (error suppression) and `&$x` keep the operand's type.
        SyntaxKind::At | SyntaxKind::Ampersand => operand,
        _ => TypeId::mixed(db),
    }
}

/// `$x++` / `$x--` read the operand before mutation.
pub(crate) fn postfix_type<'db>(db: &'db dyn salsa::Database, operand: TypeId<'db>) -> TypeId<'db> {
    numeric_preserving(db, operand)
}

/// int stays int, float stays float, everything else is `int|float`.
fn numeric_preserving<'db>(db: &'db dyn salsa::Database, operand: TypeId<'db>) -> TypeId<'db> {
    if is_int(db, operand) {
        TypeId::int(db)
    } else if is_float(db, operand) {
        TypeId::float(db)
    } else {
        int_or_float(db)
    }
}

/// Both operands decidedly int (never float)?
fn arithmetic<'db>(
    db: &'db dyn salsa::Database,
    lhs: TypeId<'db>,
    rhs: TypeId<'db>,
) -> TypeId<'db> {
    if is_int(db, lhs) && is_int(db, rhs) {
        TypeId::int(db)
    } else if is_float(db, lhs) || is_float(db, rhs) {
        TypeId::float(db)
    } else {
        int_or_float(db)
    }
}

pub(crate) fn binary_type<'db>(
    db: &'db dyn salsa::Database,
    operator: SyntaxKind,
    lhs: TypeId<'db>,
    rhs: TypeId<'db>,
) -> TypeId<'db> {
    match operator {
        SyntaxKind::Plus if is_array(db, lhs) && is_array(db, rhs) => join(db, lhs, rhs),
        SyntaxKind::Plus | SyntaxKind::Minus | SyntaxKind::Star => arithmetic(db, lhs, rhs),
        // `/` and `**` never stay int on two ints: division can be
        // fractional, exponentiation overflows to float.
        SyntaxKind::Slash | SyntaxKind::StarStar => {
            if is_float(db, lhs) || is_float(db, rhs) {
                TypeId::float(db)
            } else {
                int_or_float(db)
            }
        }
        SyntaxKind::Percent
        | SyntaxKind::Ampersand
        | SyntaxKind::Pipe
        | SyntaxKind::Caret
        | SyntaxKind::LessLess
        | SyntaxKind::GreaterGreater => TypeId::int(db),
        SyntaxKind::Dot => TypeId::string(db),
        SyntaxKind::EqualsEquals
        | SyntaxKind::BangEquals
        | SyntaxKind::EqualsEqualsEquals
        | SyntaxKind::BangEqualsEquals
        | SyntaxKind::Less
        | SyntaxKind::Greater
        | SyntaxKind::LessEquals
        | SyntaxKind::GreaterEquals
        | SyntaxKind::AmpersandAmpersand
        | SyntaxKind::PipePipe
        | SyntaxKind::And
        | SyntaxKind::Or
        | SyntaxKind::Xor
        | SyntaxKind::InstanceOf => TypeId::bool(db),
        SyntaxKind::Spaceship => TypeId::int_range(db, Some(-1), Some(1)),
        // The walker owns `??` (environment-sensitive); this fallback
        // serves operand positions the walker does not special-case.
        // `union` drops `never` as an identity, so an all-null left
        // collapses to `rhs` on its own.
        SyntaxKind::QuestionQuestion => TypeId::union(db, [lhs.without_null(db), rhs]),
        _ => TypeId::mixed(db),
    }
}

/// An index read: shapes answer their field (or the field join when
/// the key is not a known literal), arrays their value, strings a
/// string; everything else is silence.
pub(crate) fn index_type<'db>(
    db: &'db dyn salsa::Database,
    subject: TypeId<'db>,
    index: Option<TypeId<'db>>,
) -> TypeId<'db> {
    if let Some(fields) = subject.shape_fields(db) {
        let literal_key = index.and_then(|key| {
            key.int_literal_value(db)
                .map(crate::representation::ShapeKey::Integer)
                .or_else(|| {
                    key.string_literal_value(db)
                        .map(crate::representation::ShapeKey::String)
                })
        });
        if let Some(wanted) = literal_key
            && let Some(field) = fields.iter().find(|field| field.key == wanted)
        {
            return field.value;
        }
        return fields
            .iter()
            .map(|field| field.value)
            .reduce(|left, right| join(db, left, right))
            .unwrap_or_else(|| TypeId::mixed(db));
    }
    if let Some(value) = subject.array_value(db) {
        return value;
    }
    if subject == TypeId::string(db) || subject.string_literal_value(db).is_some() {
        return TypeId::string(db);
    }
    TypeId::mixed(db)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_syntax::SyntaxKind;

    use super::*;
    use crate::representation::{ShapeField, ShapeKey, TypeId};

    #[test]
    fn literals_type_as_their_literal_forms() {
        let db = TestDatabase::default();
        assert_eq!(literal_type(&db, "42"), TypeId::int_literal(&db, 42));
        assert_eq!(literal_type(&db, "1_000"), TypeId::int_literal(&db, 1000));
        assert_eq!(literal_type(&db, "0x10"), TypeId::int_literal(&db, 16));
        assert_eq!(literal_type(&db, "0o17"), TypeId::int_literal(&db, 15));
        assert_eq!(literal_type(&db, "017"), TypeId::int_literal(&db, 15));
        assert_eq!(literal_type(&db, "0b101"), TypeId::int_literal(&db, 5));
        assert_eq!(literal_type(&db, "1.5"), TypeId::float_literal(&db, 1.5));
        assert_eq!(literal_type(&db, "1e3"), TypeId::float_literal(&db, 1000.0));
        assert_eq!(
            literal_type(&db, r"'it\''"),
            TypeId::string_literal(&db, "it'"),
            "escaped quote unescapes",
        );
        assert_eq!(
            literal_type(&db, r"'a\\b'"),
            TypeId::string_literal(&db, r"a\b"),
        );
        // An integer literal PHP overflows to float types as float.
        assert_eq!(
            literal_type(&db, "99999999999999999999"),
            TypeId::float(&db),
        );
    }

    #[test]
    fn named_atoms_type_and_constants_stay_unknown() {
        let db = TestDatabase::default();
        assert_eq!(
            named_reference_type(&db, "true"),
            Some(TypeId::bool_literal(&db, true)),
        );
        assert_eq!(
            named_reference_type(&db, "FALSE"),
            Some(TypeId::bool_literal(&db, false)),
        );
        assert_eq!(named_reference_type(&db, "Null"), Some(TypeId::null(&db)));
        assert_eq!(named_reference_type(&db, "PHP_EOL"), None);
    }

    #[test]
    fn casts_type_by_their_operator() {
        let db = TestDatabase::default();
        let mixed = TypeId::mixed(&db);
        assert_eq!(cast_type(&db, SyntaxKind::IntCast, mixed), TypeId::int(&db));
        assert_eq!(
            cast_type(&db, SyntaxKind::BoolCast, mixed),
            TypeId::bool(&db)
        );
        assert_eq!(
            cast_type(&db, SyntaxKind::FloatCast, mixed),
            TypeId::float(&db),
        );
        assert_eq!(
            cast_type(&db, SyntaxKind::StringCast, mixed),
            TypeId::string(&db),
        );
        assert_eq!(
            cast_type(&db, SyntaxKind::BinaryCast, mixed),
            TypeId::string(&db),
        );
        assert_eq!(
            cast_type(&db, SyntaxKind::ObjectCast, mixed),
            TypeId::object(&db),
        );
        // (array) on an array keeps it; on anything else, general array.
        let list = TypeId::list(&db, TypeId::int(&db));
        assert_eq!(cast_type(&db, SyntaxKind::ArrayCast, list), list);
        assert_eq!(
            cast_type(&db, SyntaxKind::ArrayCast, mixed),
            TypeId::array(
                &db,
                TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]),
                TypeId::mixed(&db),
            ),
        );
    }

    #[test]
    fn unary_and_postfix_operators_type() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let float = TypeId::float(&db);
        let mixed = TypeId::mixed(&db);
        let int_or_float = TypeId::union(&db, [int, float]);
        assert_eq!(unary_type(&db, SyntaxKind::Bang, mixed), TypeId::bool(&db));
        assert_eq!(unary_type(&db, SyntaxKind::Tilde, mixed), int);
        assert_eq!(unary_type(&db, SyntaxKind::Minus, int), int);
        assert_eq!(unary_type(&db, SyntaxKind::Minus, float), float);
        assert_eq!(unary_type(&db, SyntaxKind::Minus, mixed), int_or_float);
        assert_eq!(
            unary_type(&db, SyntaxKind::Minus, TypeId::int_literal(&db, 1)),
            TypeId::int_literal(&db, -1),
            "negative literals feed narrowing",
        );
        // `@$x` keeps the operand's type; `&$x` too.
        assert_eq!(unary_type(&db, SyntaxKind::At, float), float);
        assert_eq!(postfix_type(&db, int), int);
        assert_eq!(postfix_type(&db, mixed), int_or_float);
    }

    #[test]
    fn binary_operators_type_by_the_table() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let float = TypeId::float(&db);
        let string = TypeId::string(&db);
        let mixed = TypeId::mixed(&db);
        let bool_type = TypeId::bool(&db);
        let int_or_float = TypeId::union(&db, [int, float]);

        assert_eq!(binary_type(&db, SyntaxKind::Plus, int, int), int);
        assert_eq!(binary_type(&db, SyntaxKind::Plus, int, float), float);
        assert_eq!(binary_type(&db, SyntaxKind::Star, mixed, int), int_or_float);
        assert_eq!(binary_type(&db, SyntaxKind::Slash, int, int), int_or_float);
        assert_eq!(binary_type(&db, SyntaxKind::Slash, float, int), float);
        assert_eq!(binary_type(&db, SyntaxKind::Percent, mixed, mixed), int);
        assert_eq!(
            binary_type(&db, SyntaxKind::StarStar, int, int),
            int_or_float,
            "exponentiation overflows to float",
        );
        // `+` on two arrays is the array-union operator.
        let left = TypeId::list(&db, int);
        let right = TypeId::list(&db, string);
        assert_eq!(
            binary_type(&db, SyntaxKind::Plus, left, right),
            crate::widening::join(&db, left, right),
        );
        assert_eq!(binary_type(&db, SyntaxKind::Dot, mixed, mixed), string);
        assert_eq!(
            binary_type(&db, SyntaxKind::EqualsEqualsEquals, mixed, mixed),
            bool_type,
        );
        assert_eq!(binary_type(&db, SyntaxKind::Less, mixed, mixed), bool_type);
        assert_eq!(
            binary_type(&db, SyntaxKind::Spaceship, mixed, mixed),
            TypeId::int_range(&db, Some(-1), Some(1)),
        );
        assert_eq!(
            binary_type(&db, SyntaxKind::AmpersandAmpersand, mixed, mixed),
            bool_type,
        );
        assert_eq!(binary_type(&db, SyntaxKind::And, mixed, mixed), bool_type);
        assert_eq!(
            binary_type(&db, SyntaxKind::InstanceOf, mixed, mixed),
            bool_type,
        );
        assert_eq!(
            binary_type(&db, SyntaxKind::Ampersand, int, int),
            int,
            "bitwise on integers",
        );
        assert_eq!(binary_type(&db, SyntaxKind::LessLess, mixed, mixed), int);
        // The walker owns `??`; the fallback here is the null-stripped join.
        let nullable_int = TypeId::union(&db, [int, TypeId::null(&db)]);
        assert_eq!(
            binary_type(&db, SyntaxKind::QuestionQuestion, nullable_int, string),
            TypeId::union(&db, [int, string]),
        );
        // Anything unknown answers mixed (the pipe operator, for one).
        assert_eq!(
            binary_type(&db, SyntaxKind::PipeGreater, mixed, mixed),
            mixed,
        );
    }

    #[test]
    fn index_reads_type_by_the_subject() {
        let db = TestDatabase::default();
        let int = TypeId::int(&db);
        let string = TypeId::string(&db);
        let mixed = TypeId::mixed(&db);
        let list = TypeId::list(&db, string);
        assert_eq!(index_type(&db, list, Some(int)), string);
        assert_eq!(index_type(&db, string, Some(int)), string);
        assert_eq!(index_type(&db, mixed, Some(int)), mixed);
        // A shape with a known literal key answers the field.
        let shape = TypeId::shape(
            &db,
            vec![ShapeField {
                key: ShapeKey::String("id".to_owned()),
                optional: false,
                value: int,
            }],
        );
        assert_eq!(
            index_type(&db, shape, Some(TypeId::string_literal(&db, "id"))),
            int,
        );
        assert_eq!(index_type(&db, shape, Some(string)), int, "join of fields");
    }
}
