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
const ASSIGNMENT_LEVEL: u8 = 9;
const TERNARY_LEVEL: u8 = 10;
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

fn is_assignment_operator(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Equals
            | SyntaxKind::PlusEquals
            | SyntaxKind::MinusEquals
            | SyntaxKind::StarEquals
            | SyntaxKind::SlashEquals
            | SyntaxKind::DotEquals
            | SyntaxKind::PercentEquals
            | SyntaxKind::StarStarEquals
            | SyntaxKind::AmpersandEquals
            | SyntaxKind::PipeEquals
            | SyntaxKind::CaretEquals
            | SyntaxKind::LessLessEquals
            | SyntaxKind::GreaterGreaterEquals
            | SyntaxKind::QuestionQuestionEquals
    )
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
            | SyntaxKind::Identifier
            | SyntaxKind::Backslash
            | SyntaxKind::Namespace
            | SyntaxKind::Static
            | SyntaxKind::Dollar
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
        if is_assignment_operator(kind) {
            let power = left_binding_power(ASSIGNMENT_LEVEL);
            if power < minimum_power {
                break;
            }
            let marker = left.precede(parser);
            parser.bump();
            if kind == SyntaxKind::Equals {
                // `= &$value` assigns by reference.
                parser.eat(SyntaxKind::Ampersand);
            }
            expression_with_minimum_power(parser, power - 1);
            left = marker.complete(parser, SyntaxKind::AssignmentExpression);
            previous_power = Some(power);
            continue;
        }
        if kind == SyntaxKind::Question {
            let power = left_binding_power(TERNARY_LEVEL);
            if power < minimum_power {
                break;
            }
            // Zend rejects unparenthesized ternary chains since 8.0.
            if previous_power == Some(power) {
                parser.diagnose_current(ParserDiagnosticKind::NonAssociativeOperator);
            }
            let marker = left.precede(parser);
            parser.bump();
            if !parser.at(SyntaxKind::Colon) {
                // The middle operand is a full expression, as in Zend's
                // `expr '?' expr ':' expr`.
                expression(parser);
            }
            parser.expect(SyntaxKind::Colon);
            expression_with_minimum_power(parser, power + 1);
            left = marker.complete(parser, SyntaxKind::TernaryExpression);
            previous_power = Some(power);
            continue;
        }
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
            Some(SyntaxKind::OpenParenthesis) => {
                let marker = left.precede(parser);
                argument_list(parser);
                marker.complete(parser, SyntaxKind::CallExpression)
            }
            _ => break,
        };
    }
    Some(left)
}

/// One token no rule accepts inside a delimited construct: wrapped,
/// diagnosed, consumed, so every list loop makes progress.
pub(super) fn error_element(parser: &mut Parser) {
    let marker = parser.start();
    parser.diagnose_current(ParserDiagnosticKind::UnexpectedToken);
    parser.bump();
    marker.complete(parser, SyntaxKind::ErrorNode);
}

fn starts_argument(parser: &Parser) -> bool {
    parser.current().is_some_and(|kind| {
        starts_expression(kind)
            || kind == SyntaxKind::Ellipsis
            || kind == SyntaxKind::Ampersand
            || (kind.is_keyword() && parser.nth(1) == Some(SyntaxKind::Colon))
    })
}

/// After a list element: eat the separating comma, or diagnose one
/// missing unless the list sits at a legitimate boundary (its closer,
/// a statement boundary, end of input). Shared by every
/// comma-separated list of this plan.
fn expect_list_separator(parser: &mut Parser, closing: SyntaxKind) {
    if parser.eat(SyntaxKind::Comma) {
        return;
    }
    if parser.at(closing)
        || parser.at(SyntaxKind::Semicolon)
        || parser.at(SyntaxKind::CloseTag)
        || parser.at_end()
    {
        return;
    }
    parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Comma));
}

/// `( argument, ... )`. The caller has already checked the opening
/// parenthesis is there (or wants its absence diagnosed). Recovery: an
/// unexpected token is wrapped and consumed; `;`, `?>`, and end of
/// input abort the list so a runaway call cannot swallow the file.
///
/// Progress is enforced mechanically, not only by convention: the
/// nesting guard (`Parser::enter_nesting`) can refuse an argument's
/// expression outright, without consuming a token, when it fires while
/// this very loop is itself nested deep inside a pathological chain of
/// parenthesized expressions (the leftover, unconsumed `(` surfaces
/// through the postfix loop at every level that chain unwinds through,
/// not only at the top, so `argument_list` can be entered again while
/// the nesting budget is still exhausted). Trusting `argument` to
/// always consume would let such a case spin forever; instead each
/// iteration records the position before parsing an element and, if it
/// is unchanged afterward, forces an `error_element` bump.
fn argument_list(parser: &mut Parser) {
    let marker = parser.start();
    parser.expect(SyntaxKind::OpenParenthesis);
    while !parser.at(SyntaxKind::CloseParenthesis) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        // `f(...)`: the first-class callable form, a lone ellipsis.
        if parser.at(SyntaxKind::Ellipsis) && parser.nth(1) == Some(SyntaxKind::CloseParenthesis) {
            parser.bump();
            break;
        }
        if !starts_argument(parser) {
            error_element(parser);
            continue;
        }
        let position_before_element = parser.position();
        argument(parser);
        expect_list_separator(parser, SyntaxKind::CloseParenthesis);
        if parser.position() == position_before_element {
            error_element(parser);
        }
    }
    parser.expect(SyntaxKind::CloseParenthesis);
    marker.complete(parser, SyntaxKind::ArgumentList);
}

fn argument(parser: &mut Parser) {
    let marker = parser.start();
    // A named argument: `label:` where the label is an identifier or
    // any keyword (semi-reserved, accepted wholesale). `::` cannot be
    // confused with the label colon because it lexes as one token.
    let at_label = parser
        .current()
        .is_some_and(|kind| kind == SyntaxKind::Identifier || kind.is_keyword())
        && parser.nth(1) == Some(SyntaxKind::Colon);
    if at_label {
        parser.bump();
        parser.bump();
    }
    parser.eat(SyntaxKind::Ellipsis);
    // Call-site by-reference: removed from PHP, still analyzable.
    parser.eat(SyntaxKind::Ampersand);
    expression(parser);
    marker.complete(parser, SyntaxKind::Argument);
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
        Some(SyntaxKind::Variable | SyntaxKind::Dollar) => simple_variable(parser),
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash) => {
            let marker = parser.start();
            name(parser);
            Some(marker.complete(parser, SyntaxKind::NameExpression))
        }
        Some(SyntaxKind::Namespace) if parser.nth(1) == Some(SyntaxKind::Backslash) => {
            let marker = parser.start();
            name(parser);
            Some(marker.complete(parser, SyntaxKind::NameExpression))
        }
        // `static` as a scoped-access subject (`static::create()`).
        // Static closures take a different arm in task 15.
        Some(SyntaxKind::Static) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::NameExpression))
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

/// `Foo`, `Foo\Bar`, `\Foo`, `namespace\Foo`: one `Name` node. Zend
/// lexes each qualified form as a single token, which forbids interior
/// whitespace; the trivia-free token view cannot see adjacency, so
/// spaced segments parse here and adjacency is judged upstairs.
fn name(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    if parser.eat(SyntaxKind::Namespace) {
        parser.expect(SyntaxKind::Backslash);
    } else {
        parser.eat(SyntaxKind::Backslash);
    }
    parser.expect(SyntaxKind::Identifier);
    while parser.at(SyntaxKind::Backslash) && parser.nth(1) == Some(SyntaxKind::Identifier) {
        parser.bump();
        parser.bump();
    }
    marker.complete(parser, SyntaxKind::Name)
}

/// `$name`, `$$name`, `${expression}`: the dynamic forms recurse, so
/// the nesting guard applies. Returns `None` without consuming when
/// the current token is not a variable form.
fn simple_variable(parser: &mut Parser) -> Option<CompletedMarker> {
    match parser.current() {
        Some(SyntaxKind::Variable) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::VariableReference))
        }
        Some(SyntaxKind::Dollar) => {
            if !parser.enter_nesting() {
                return None;
            }
            let marker = parser.start();
            parser.bump();
            match parser.current() {
                Some(SyntaxKind::OpenBrace) => {
                    parser.bump();
                    expression(parser);
                    parser.expect(SyntaxKind::CloseBrace);
                }
                Some(SyntaxKind::Variable | SyntaxKind::Dollar) => {
                    simple_variable(parser);
                }
                _ => parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression),
            }
            let completed = marker.complete(parser, SyntaxKind::DynamicVariableExpression);
            parser.leave_nesting();
            Some(completed)
        }
        _ => None,
    }
}
