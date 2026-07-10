//! The expression grammar: a Pratt loop over the precedence table
//! transcribed from php-src's `zend_language_parser.y` (branch
//! PHP-8.5), a prefix dispatch, a postfix loop, and the primary
//! expressions. Levels are loosest first; binding powers are
//! `level * 2`, so right-associative operators can recurse at
//! `level * 2 - 1` and everything stays in one `u8` space.

use crate::diagnostic::ParserDiagnosticKind;
use crate::syntax_kind::SyntaxKind;

use super::{CompletedMarker, Parser};

// One constant per level of the precedence table (see the plan's
// authoritative table). Declared as each task consumes them, so the
// dead-code lint stays green at every commit: this task declares the
// binary levels; task 4 adds 24 and 26, task 5 adds 9 and 10, task 11
// adds 28, task 13 adds 4 through 8.
const LOGICAL_OR_LEVEL: u8 = 1;
const LOGICAL_XOR_LEVEL: u8 = 2;
const LOGICAL_AND_LEVEL: u8 = 3;
const COALESCE_LEVEL: u8 = 11;
const BOOLEAN_OR_LEVEL: u8 = 12;
const BOOLEAN_AND_LEVEL: u8 = 13;
const BITWISE_OR_LEVEL: u8 = 14;
const BITWISE_XOR_LEVEL: u8 = 15;
const BITWISE_AND_LEVEL: u8 = 16;
const EQUALITY_LEVEL: u8 = 17;
const RELATIONAL_LEVEL: u8 = 18;
const PIPE_LEVEL: u8 = 19;
const CONCATENATION_LEVEL: u8 = 20;
const SHIFT_LEVEL: u8 = 21;
const ADDITIVE_LEVEL: u8 = 22;
const MULTIPLICATIVE_LEVEL: u8 = 23;
const LOGICAL_NOT_LEVEL: u8 = 24;
const INSTANCEOF_LEVEL: u8 = 25;
const UNARY_LEVEL: u8 = 26;
const POWER_LEVEL: u8 = 27;

fn left_binding_power(level: u8) -> u8 {
    level * 2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Associativity {
    Left,
    Right,
    /// Zend rejects same-level chains; we parse them left-associatively
    /// and diagnose.
    NonAssociative,
}

/// The binary operator table. Returns the level and associativity, or
/// `None` for any token that is not a binary operator.
fn binary_operator(kind: SyntaxKind) -> Option<(u8, Associativity)> {
    use Associativity::{Left, NonAssociative, Right};
    let entry = match kind {
        SyntaxKind::Or => (LOGICAL_OR_LEVEL, Left),
        SyntaxKind::Xor => (LOGICAL_XOR_LEVEL, Left),
        SyntaxKind::And => (LOGICAL_AND_LEVEL, Left),
        SyntaxKind::QuestionQuestion => (COALESCE_LEVEL, Right),
        SyntaxKind::PipePipe => (BOOLEAN_OR_LEVEL, Left),
        SyntaxKind::AmpersandAmpersand => (BOOLEAN_AND_LEVEL, Left),
        SyntaxKind::Pipe => (BITWISE_OR_LEVEL, Left),
        SyntaxKind::Caret => (BITWISE_XOR_LEVEL, Left),
        SyntaxKind::Ampersand => (BITWISE_AND_LEVEL, Left),
        SyntaxKind::EqualsEquals
        | SyntaxKind::BangEquals
        | SyntaxKind::EqualsEqualsEquals
        | SyntaxKind::BangEqualsEquals
        | SyntaxKind::Spaceship => (EQUALITY_LEVEL, NonAssociative),
        SyntaxKind::Less
        | SyntaxKind::LessEquals
        | SyntaxKind::Greater
        | SyntaxKind::GreaterEquals => (RELATIONAL_LEVEL, NonAssociative),
        SyntaxKind::PipeGreater => (PIPE_LEVEL, Left),
        SyntaxKind::Dot => (CONCATENATION_LEVEL, Left),
        SyntaxKind::LessLess | SyntaxKind::GreaterGreater => (SHIFT_LEVEL, Left),
        SyntaxKind::Plus | SyntaxKind::Minus => (ADDITIVE_LEVEL, Left),
        SyntaxKind::Star | SyntaxKind::Slash | SyntaxKind::Percent => (MULTIPLICATIVE_LEVEL, Left),
        SyntaxKind::InstanceOf => (INSTANCEOF_LEVEL, NonAssociative),
        SyntaxKind::StarStar => (POWER_LEVEL, Right),
        _ => return None,
    };
    Some(entry)
}

/// Which tokens can start an expression. Grows with every task of this
/// plan; the statement dispatcher keys off it.
pub(super) fn starts_expression(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IntegerLiteral
            | SyntaxKind::FloatLiteral
            | SyntaxKind::SingleQuotedString
            | SyntaxKind::Variable
            | SyntaxKind::OpenParenthesis
            | SyntaxKind::Bang
            | SyntaxKind::Tilde
            | SyntaxKind::Plus
            | SyntaxKind::Minus
            | SyntaxKind::PlusPlus
            | SyntaxKind::MinusMinus
            | SyntaxKind::At
            | SyntaxKind::IntCast
            | SyntaxKind::BoolCast
            | SyntaxKind::FloatCast
            | SyntaxKind::StringCast
            | SyntaxKind::BinaryCast
            | SyntaxKind::ArrayCast
            | SyntaxKind::ObjectCast
    )
}

pub(super) fn expression(parser: &mut Parser) -> Option<CompletedMarker> {
    expression_with_minimum_power(parser, 0)
}

fn expression_with_minimum_power(
    parser: &mut Parser,
    minimum_power: u8,
) -> Option<CompletedMarker> {
    if !parser.enter_nesting() {
        return None;
    }
    let result = binary_loop(parser, minimum_power);
    parser.leave_nesting();
    result
}

fn binary_loop(parser: &mut Parser, minimum_power: u8) -> Option<CompletedMarker> {
    let mut left = prefix_expression(parser)?;
    // The previous wrap's binding power, for the non-associativity
    // diagnostic: a same-level wrap right after a non-associative one
    // is a chain Zend rejects.
    let mut previous_power: Option<u8> = None;
    while let Some(kind) = parser.current() {
        let Some((level, associativity)) = binary_operator(kind) else {
            break;
        };
        let power = left_binding_power(level);
        if power < minimum_power {
            break;
        }
        if associativity == Associativity::NonAssociative && previous_power == Some(power) {
            parser.diagnose_current(ParserDiagnosticKind::NonAssociativeOperator);
        }
        let marker = left.precede(parser);
        parser.bump();
        let next_minimum = match associativity {
            Associativity::Right => power - 1,
            Associativity::Left | Associativity::NonAssociative => power + 1,
        };
        // A missing right operand was already diagnosed downstream; the
        // node still completes so the tree stays full-coverage.
        expression_with_minimum_power(parser, next_minimum);
        left = marker.complete(parser, SyntaxKind::BinaryExpression);
        previous_power = Some(power);
    }
    Some(left)
}

fn prefix_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    let Some(kind) = parser.current() else {
        parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
        return None;
    };
    let (node_kind, operand_power) = match kind {
        SyntaxKind::Bang => (
            SyntaxKind::PrefixExpression,
            left_binding_power(LOGICAL_NOT_LEVEL),
        ),
        SyntaxKind::Plus
        | SyntaxKind::Minus
        | SyntaxKind::Tilde
        | SyntaxKind::At
        | SyntaxKind::PlusPlus
        | SyntaxKind::MinusMinus => (
            SyntaxKind::PrefixExpression,
            left_binding_power(UNARY_LEVEL),
        ),
        SyntaxKind::IntCast
        | SyntaxKind::BoolCast
        | SyntaxKind::FloatCast
        | SyntaxKind::StringCast
        | SyntaxKind::BinaryCast
        | SyntaxKind::ArrayCast
        | SyntaxKind::ObjectCast => (SyntaxKind::CastExpression, left_binding_power(UNARY_LEVEL)),
        _ => return postfix_expression(parser),
    };
    let marker = parser.start();
    parser.bump();
    // A missing operand is diagnosed downstream; the node completes
    // regardless, partial trees are normal citizens.
    expression_with_minimum_power(parser, operand_power);
    Some(marker.complete(parser, node_kind))
}

/// The tightest tier: postfix wraps applied greedily around a primary.
/// Tasks 7 and 8 add call, member, scoped, and index arms to this loop.
fn postfix_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    let mut left = primary_expression(parser)?;
    loop {
        left = match parser.current() {
            Some(SyntaxKind::PlusPlus | SyntaxKind::MinusMinus) => {
                let marker = left.precede(parser);
                parser.bump();
                marker.complete(parser, SyntaxKind::PostfixExpression)
            }
            _ => break,
        };
    }
    Some(left)
}

fn primary_expression(parser: &mut Parser) -> Option<CompletedMarker> {
    match parser.current() {
        Some(
            SyntaxKind::IntegerLiteral | SyntaxKind::FloatLiteral | SyntaxKind::SingleQuotedString,
        ) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::Literal))
        }
        Some(SyntaxKind::Variable) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::VariableReference))
        }
        Some(SyntaxKind::OpenParenthesis) => Some(parenthesized_expression(parser)),
        _ => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
            None
        }
    }
}

fn parenthesized_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump();
    expression(parser);
    parser.expect(SyntaxKind::CloseParenthesis);
    marker.complete(parser, SyntaxKind::ParenthesizedExpression)
}
