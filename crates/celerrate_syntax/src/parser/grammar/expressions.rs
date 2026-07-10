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
const PRINT_LEVEL: u8 = 4;
const YIELD_LEVEL: u8 = 5;
const YIELD_FROM_LEVEL: u8 = 6;
const THROW_LEVEL: u8 = 7;
const INCLUDE_LEVEL: u8 = 8;
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
const CLONE_LEVEL: u8 = 28;

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
            | SyntaxKind::OpenBracket
            | SyntaxKind::Array
            | SyntaxKind::List
            | SyntaxKind::DoubleQuote
            | SyntaxKind::Backtick
            | SyntaxKind::HeredocStart
            | SyntaxKind::New
            | SyntaxKind::Clone
            | SyntaxKind::Isset
            | SyntaxKind::Empty
            | SyntaxKind::Eval
            | SyntaxKind::Exit
            | SyntaxKind::Print
            | SyntaxKind::Throw
            | SyntaxKind::Yield
            | SyntaxKind::YieldFrom
            | SyntaxKind::Include
            | SyntaxKind::IncludeOnce
            | SyntaxKind::Require
            | SyntaxKind::RequireOnce
            | SyntaxKind::Match
            | SyntaxKind::Function
            | SyntaxKind::Fn
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
        SyntaxKind::Clone if parser.nth(1) != Some(SyntaxKind::OpenParenthesis) => {
            let marker = parser.start();
            parser.bump();
            expression_with_minimum_power(parser, left_binding_power(CLONE_LEVEL));
            return Some(marker.complete(parser, SyntaxKind::CloneExpression));
        }
        SyntaxKind::Yield => return Some(yield_expression(parser)),
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
        SyntaxKind::Print => (SyntaxKind::PrintExpression, left_binding_power(PRINT_LEVEL)),
        SyntaxKind::Throw => (SyntaxKind::ThrowExpression, left_binding_power(THROW_LEVEL)),
        SyntaxKind::Include
        | SyntaxKind::IncludeOnce
        | SyntaxKind::Require
        | SyntaxKind::RequireOnce => (
            SyntaxKind::IncludeExpression,
            left_binding_power(INCLUDE_LEVEL),
        ),
        SyntaxKind::YieldFrom => (
            SyntaxKind::YieldExpression,
            left_binding_power(YIELD_FROM_LEVEL),
        ),
        _ => return postfix_expression(parser),
    };
    let marker = parser.start();
    parser.bump();
    // A missing operand is diagnosed downstream; the node completes
    // regardless, partial trees are normal citizens.
    expression_with_minimum_power(parser, operand_power);
    Some(marker.complete(parser, node_kind))
}

/// `yield`, `yield value`, `yield key => value`. The operand is
/// optional: a bare `yield` is a complete expression.
fn yield_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // `yield`
    if parser.current().is_some_and(starts_expression) {
        expression_with_minimum_power(parser, left_binding_power(YIELD_LEVEL));
        if parser.eat(SyntaxKind::FatArrow) {
            expression_with_minimum_power(parser, left_binding_power(YIELD_LEVEL));
        }
    }
    marker.complete(parser, SyntaxKind::YieldExpression)
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
            Some(SyntaxKind::Arrow | SyntaxKind::NullsafeArrow) => {
                let marker = left.precede(parser);
                parser.bump();
                member_name(parser);
                marker.complete(parser, SyntaxKind::MemberAccessExpression)
            }
            Some(SyntaxKind::ColonColon) => {
                let marker = left.precede(parser);
                parser.bump();
                member_name(parser);
                marker.complete(parser, SyntaxKind::ScopedAccessExpression)
            }
            Some(SyntaxKind::OpenBracket) => {
                let marker = left.precede(parser);
                parser.bump();
                if !parser.at(SyntaxKind::CloseBracket) {
                    expression(parser);
                }
                parser.expect(SyntaxKind::CloseBracket);
                marker.complete(parser, SyntaxKind::IndexExpression)
            }
            _ => break,
        };
    }
    Some(left)
}

/// The name after `->`, `?->`, or `::`: an identifier, any keyword
/// (Zend's semi-reserved list accepted wholesale, `::class` included;
/// per-position validity is semantic), a variable form, or
/// `{ expression }`.
fn member_name(parser: &mut Parser) {
    let marker = parser.start();
    match parser.current() {
        Some(kind) if kind == SyntaxKind::Identifier || kind.is_keyword() => parser.bump(),
        Some(SyntaxKind::Variable | SyntaxKind::Dollar) => {
            simple_variable(parser);
        }
        Some(SyntaxKind::OpenBrace) => {
            parser.bump();
            expression(parser);
            parser.expect(SyntaxKind::CloseBrace);
        }
        _ => parser.diagnose_current(ParserDiagnosticKind::ExpectedMemberName),
    }
    marker.complete(parser, SyntaxKind::MemberName);
}

/// One token no rule accepts inside a delimited construct: wrapped,
/// diagnosed, consumed, so every list loop makes progress.
pub(super) fn error_element(parser: &mut Parser) {
    let marker = parser.start();
    parser.diagnose_current(ParserDiagnosticKind::UnexpectedToken);
    parser.bump();
    marker.complete(parser, SyntaxKind::ErrorNode);
}

fn starts_argument(parser: &mut Parser) -> bool {
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

/// `[ elements ]` or `array( elements )`: one node kind, the delimiter
/// tokens tell the forms apart. Also the destructuring target shape;
/// the parser does not distinguish a literal from an assignment target.
fn array_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    if parser.eat(SyntaxKind::Array) {
        if parser.at(SyntaxKind::OpenParenthesis) {
            parser.bump();
            array_element_list(parser, SyntaxKind::CloseParenthesis);
            parser.expect(SyntaxKind::CloseParenthesis);
        } else {
            parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
        }
    } else {
        parser.bump(); // `[`
        array_element_list(parser, SyntaxKind::CloseBracket);
        parser.expect(SyntaxKind::CloseBracket);
    }
    marker.complete(parser, SyntaxKind::ArrayExpression)
}

/// `list( elements )`, the keyword destructuring form.
fn list_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // `list`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        array_element_list(parser, SyntaxKind::CloseParenthesis);
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    marker.complete(parser, SyntaxKind::ListExpression)
}

/// Elements until `closing`. Same recovery contract as `argument_list`:
/// unexpected tokens are wrapped and consumed; `;`, `?>`, and end of
/// input abort.
///
/// Progress is enforced mechanically, not only by convention, for the
/// same reason `argument_list` needs it: the nesting guard can refuse
/// an element's expression outright, without consuming a token, when it
/// fires while this very loop is nested deep inside a pathological
/// chain of `[` (the leftover, unconsumed `[` surfaces through the
/// postfix loop at every level that chain unwinds through, not only at
/// the top, so `array_element_list` can be entered again while the
/// nesting budget is still exhausted). Trusting `array_element` to
/// always consume would let such a case spin forever; instead each
/// iteration records the position before parsing an element and, if it
/// is unchanged afterward, forces an `error_element` bump.
fn array_element_list(parser: &mut Parser, closing: SyntaxKind) {
    while !parser.at(closing) && !parser.at_end() {
        if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
            break;
        }
        if parser.at(SyntaxKind::Comma) {
            // An empty destructuring slot: the comma stands alone.
            parser.bump();
            continue;
        }
        if !starts_array_element(parser) {
            error_element(parser);
            continue;
        }
        let position_before_element = parser.position();
        array_element(parser);
        expect_list_separator(parser, closing);
        if parser.position() == position_before_element {
            error_element(parser);
        }
    }
}

fn starts_array_element(parser: &mut Parser) -> bool {
    parser.current().is_some_and(|kind| {
        starts_expression(kind) || kind == SyntaxKind::Ellipsis || kind == SyntaxKind::Ampersand
    })
}

fn array_element(parser: &mut Parser) {
    let marker = parser.start();
    parser.eat(SyntaxKind::Ellipsis);
    parser.eat(SyntaxKind::Ampersand);
    expression(parser);
    if parser.eat(SyntaxKind::FatArrow) {
        parser.eat(SyntaxKind::Ampersand);
        expression(parser);
    }
    marker.complete(parser, SyntaxKind::ArrayElement);
}

/// `"..."`, heredocs, and backticks share one loop: fragments stay
/// tokens, interpolations become nodes. The lexer already diagnosed an
/// unterminated string, so the missing closer is eaten silently here.
fn interpolated_string(
    parser: &mut Parser,
    closing: SyntaxKind,
    node_kind: SyntaxKind,
) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // the opening delimiter
    while !parser.at(closing) && !parser.at_end() {
        match parser.current() {
            Some(SyntaxKind::StringFragment) => parser.bump(),
            Some(SyntaxKind::Variable) => simple_interpolation(parser),
            Some(SyntaxKind::OpenBrace) => brace_interpolation(parser),
            Some(SyntaxKind::DollarOpenBrace) => dollar_brace_interpolation(parser),
            // The lexer does not produce anything else between string
            // delimiters; this arm is fuzz armor, not grammar.
            _ => error_element(parser),
        }
    }
    parser.eat(closing);
    marker.complete(parser, node_kind)
}

/// Zend's "simple" interpolation: the variable, then at most one
/// property hop or one offset. The lexer only emits the arrow or the
/// bracket when the full form is present; the diagnostics are
/// defensive, for fuzzed streams.
fn simple_interpolation(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // the variable
    if parser.at(SyntaxKind::Arrow) || parser.at(SyntaxKind::NullsafeArrow) {
        parser.bump();
        if !parser.eat(SyntaxKind::Identifier) {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedMemberName);
        }
    } else if parser.eat(SyntaxKind::OpenBracket) {
        parser.eat(SyntaxKind::Minus);
        if matches!(
            parser.current(),
            Some(SyntaxKind::IntegerLiteral | SyntaxKind::Identifier | SyntaxKind::Variable)
        ) {
            parser.bump();
        } else {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
        }
        parser.expect(SyntaxKind::CloseBracket);
    }
    marker.complete(parser, SyntaxKind::SimpleInterpolation);
}

fn brace_interpolation(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `{`
    expression(parser);
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::BraceInterpolation);
}

fn dollar_brace_interpolation(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `${`
    expression(parser);
    parser.expect(SyntaxKind::CloseBrace);
    marker.complete(parser, SyntaxKind::DollarBraceInterpolation);
}

/// A keyword followed by a mandatory argument list (`isset`, `empty`,
/// `eval`). Arity and argument validity are semantic; the shared list
/// brings its recovery along.
fn keyword_call(parser: &mut Parser, node_kind: SyntaxKind) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // the keyword
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    marker.complete(parser, node_kind)
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
        Some(SyntaxKind::OpenBracket | SyntaxKind::Array) => Some(array_expression(parser)),
        Some(SyntaxKind::List) => Some(list_expression(parser)),
        Some(SyntaxKind::DoubleQuote) => Some(interpolated_string(
            parser,
            SyntaxKind::DoubleQuote,
            SyntaxKind::InterpolatedString,
        )),
        Some(SyntaxKind::Backtick) => Some(interpolated_string(
            parser,
            SyntaxKind::Backtick,
            SyntaxKind::ShellExecExpression,
        )),
        Some(SyntaxKind::HeredocStart) => Some(interpolated_string(
            parser,
            SyntaxKind::HeredocEnd,
            SyntaxKind::HeredocExpression,
        )),
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
        Some(SyntaxKind::Function | SyntaxKind::Fn) => Some(closure_or_arrow_function(parser)),
        Some(SyntaxKind::Static)
            if matches!(parser.nth(1), Some(SyntaxKind::Function | SyntaxKind::Fn)) =>
        {
            Some(closure_or_arrow_function(parser))
        }
        // `static` as a scoped-access subject (`static::create()`).
        Some(SyntaxKind::Static) => {
            let marker = parser.start();
            parser.bump();
            Some(marker.complete(parser, SyntaxKind::NameExpression))
        }
        Some(SyntaxKind::OpenParenthesis) => Some(parenthesized_expression(parser)),
        Some(SyntaxKind::New) => Some(new_expression(parser)),
        // The 8.5 function form; a primary, so postfix chains wrap it.
        Some(SyntaxKind::Clone) if parser.nth(1) == Some(SyntaxKind::OpenParenthesis) => {
            let marker = parser.start();
            parser.bump();
            argument_list(parser);
            Some(marker.complete(parser, SyntaxKind::CloneExpression))
        }
        Some(SyntaxKind::Isset) => Some(keyword_call(parser, SyntaxKind::IssetExpression)),
        Some(SyntaxKind::Empty) => Some(keyword_call(parser, SyntaxKind::EmptyExpression)),
        Some(SyntaxKind::Eval) => Some(keyword_call(parser, SyntaxKind::EvalExpression)),
        Some(SyntaxKind::Exit) => {
            let marker = parser.start();
            parser.bump();
            if parser.at(SyntaxKind::OpenParenthesis) {
                argument_list(parser);
            }
            Some(marker.complete(parser, SyntaxKind::ExitExpression))
        }
        Some(SyntaxKind::Match) => Some(match_expression(parser)),
        _ => {
            parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression);
            None
        }
    }
}

/// `new` with a class reference and optional arguments. The class
/// reference is narrower than an expression: calls are excluded so
/// the argument list stays the `new`'s. Postfix chains wrap the
/// completed node afterwards, which is how 8.4's `new Foo()->bar()`
/// parses; version gating is semantic.
fn new_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // `new`
    match parser.current() {
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash) => {
            name(parser);
        }
        Some(SyntaxKind::Namespace) if parser.nth(1) == Some(SyntaxKind::Backslash) => {
            name(parser);
        }
        Some(SyntaxKind::Static) => parser.bump(),
        Some(SyntaxKind::Variable | SyntaxKind::Dollar) => {
            new_class_reference_chain(parser);
        }
        Some(SyntaxKind::OpenParenthesis) => {
            parenthesized_expression(parser);
        }
        // `new class { ... }` (anonymous classes) belongs to the
        // declarations plan; recovery keeps the tokens until then.
        Some(SyntaxKind::Class) => error_element(parser),
        _ => parser.diagnose_current(ParserDiagnosticKind::ExpectedExpression),
    }
    if parser.at(SyntaxKind::OpenParenthesis) {
        argument_list(parser);
    }
    marker.complete(parser, SyntaxKind::NewExpression)
}

/// The variable form of a class reference: member, scoped, and index
/// wraps, calls excluded (Zend's new_variable). Deliberately repeats
/// three postfix arms; folding them together would thread an
/// allow-calls flag through the hot loop for one caller.
fn new_class_reference_chain(parser: &mut Parser) -> Option<CompletedMarker> {
    let mut left = simple_variable(parser)?;
    loop {
        left = match parser.current() {
            Some(SyntaxKind::Arrow | SyntaxKind::NullsafeArrow) => {
                let marker = left.precede(parser);
                parser.bump();
                member_name(parser);
                marker.complete(parser, SyntaxKind::MemberAccessExpression)
            }
            Some(SyntaxKind::ColonColon) => {
                let marker = left.precede(parser);
                parser.bump();
                member_name(parser);
                marker.complete(parser, SyntaxKind::ScopedAccessExpression)
            }
            Some(SyntaxKind::OpenBracket) => {
                let marker = left.precede(parser);
                parser.bump();
                if !parser.at(SyntaxKind::CloseBracket) {
                    expression(parser);
                }
                parser.expect(SyntaxKind::CloseBracket);
                marker.complete(parser, SyntaxKind::IndexExpression)
            }
            _ => break,
        };
    }
    Some(left)
}

fn parenthesized_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump();
    expression(parser);
    parser.expect(SyntaxKind::CloseParenthesis);
    marker.complete(parser, SyntaxKind::ParenthesizedExpression)
}

/// `match (subject) { arms }`. Same recovery contract as the other
/// delimited lists: unexpected tokens are wrapped and consumed; `;`,
/// `?>`, and end of input abort.
///
/// Progress is enforced mechanically, not only by convention, for the
/// same reason `argument_list` needs it: the nesting guard
/// (`Parser::enter_nesting`) can refuse an arm's condition expression
/// outright, without consuming a token, when it fires while this very
/// loop is itself nested deep inside a pathological chain of
/// parenthesized expressions inside the condition (the leftover,
/// unconsumed `(` surfaces through the postfix loop at every level that
/// chain unwinds through, not only at the top, so this loop can be
/// entered again while the nesting budget is still exhausted). Trusting
/// `match_arm` to always consume would let such a case spin forever;
/// instead each iteration records the position before parsing an arm
/// and, if it is unchanged afterward, forces an `error_element` bump.
fn match_expression(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.bump(); // `match`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        expression(parser);
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    if parser.at(SyntaxKind::OpenBrace) {
        parser.bump();
        while !parser.at(SyntaxKind::CloseBrace) && !parser.at_end() {
            if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
                break;
            }
            if parser.at(SyntaxKind::Default) || parser.current().is_some_and(starts_expression) {
                let position_before_arm = parser.position();
                match_arm(parser);
                expect_list_separator(parser, SyntaxKind::CloseBrace);
                if parser.position() == position_before_arm {
                    error_element(parser);
                }
            } else {
                error_element(parser);
                continue;
            }
        }
        parser.expect(SyntaxKind::CloseBrace);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenBrace));
    }
    marker.complete(parser, SyntaxKind::MatchExpression)
}

/// One arm: a condition list (or `default`), `=>`, the body.
///
/// Unlike `match_expression`'s arm loop, this loop's own termination
/// does not depend on the position guard below: every iteration already
/// bumps the comma before attempting a condition, so it always advances
/// and is bounded by the finite token stream regardless. The guard is
/// defensive, not load-bearing: it exists so a condition the nesting
/// guard refuses (without consuming a token) is still swept into an
/// `ErrorNode` here, rather than left dangling for
/// `match_expression`'s own arm-loop guard to recover one layer out.
fn match_arm(parser: &mut Parser) {
    let marker = parser.start();
    if !parser.eat(SyntaxKind::Default) {
        // Conditions: comma-separated full expressions, up to the `=>`.
        // Any comma before the arrow separates conditions; arm-level
        // commas only occur after the body.
        expression(parser);
        while parser.at(SyntaxKind::Comma) {
            parser.bump();
            if !parser.current().is_some_and(starts_expression) {
                // A trailing comma in the condition list, or malformed
                // input right after the comma (the arrow, or the arm
                // list's closing brace on input like `match ($x) { 1, }`):
                // stop instead of forcing a parse, so that non-expression
                // token is left for the caller to consume rather than
                // being swallowed here.
                break;
            }
            let position_before_condition = parser.position();
            expression(parser);
            if parser.position() == position_before_condition {
                error_element(parser);
            }
        }
    }
    parser.expect(SyntaxKind::FatArrow);
    expression(parser);
    marker.complete(parser, SyntaxKind::MatchArm);
}

/// `function`/`fn` at expression position, optionally preceded by
/// `static`; the caller checked the shape.
fn closure_or_arrow_function(parser: &mut Parser) -> CompletedMarker {
    let marker = parser.start();
    parser.eat(SyntaxKind::Static);
    if parser.at(SyntaxKind::Function) {
        parser.bump();
        parser.eat(SyntaxKind::Ampersand); // by-reference return
        parameter_list(parser);
        if parser.at(SyntaxKind::Use) {
            closure_use_clause(parser);
        }
        if parser.eat(SyntaxKind::Colon) {
            type_reference(parser);
        }
        super::statements::block(parser);
        marker.complete(parser, SyntaxKind::ClosureExpression)
    } else {
        parser.expect(SyntaxKind::Fn);
        parser.eat(SyntaxKind::Ampersand); // by-reference return
        parameter_list(parser);
        if parser.eat(SyntaxKind::Colon) {
            type_reference(parser);
        }
        parser.expect(SyntaxKind::FatArrow);
        expression(parser);
        marker.complete(parser, SyntaxKind::ArrowFunctionExpression)
    }
}

/// `( parameter, ... )`. Progress is guaranteed without an explicit
/// committed-position guard: `starts_parameter` only admits kinds that
/// force `parameter` to consume at least one token before it can reach
/// any refusable sub-parse (the default value), unlike `argument_list`
/// where the element can be a bare, refusable expression.
fn parameter_list(parser: &mut Parser) {
    let marker = parser.start();
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        while !parser.at(SyntaxKind::CloseParenthesis) && !parser.at_end() {
            if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
                break;
            }
            if !starts_parameter(parser) {
                error_element(parser);
                continue;
            }
            parameter(parser);
            expect_list_separator(parser, SyntaxKind::CloseParenthesis);
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    marker.complete(parser, SyntaxKind::ParameterList);
}

fn starts_parameter(parser: &mut Parser) -> bool {
    matches!(
        parser.current(),
        Some(
            SyntaxKind::Variable
                | SyntaxKind::Ampersand
                | SyntaxKind::Ellipsis
                | SyntaxKind::Question
                | SyntaxKind::Identifier
                | SyntaxKind::Backslash
                | SyntaxKind::Namespace
                | SyntaxKind::Array
                | SyntaxKind::Callable
                | SyntaxKind::Static
        )
    )
}

fn parameter(parser: &mut Parser) {
    let marker = parser.start();
    if !matches!(
        parser.current(),
        Some(SyntaxKind::Variable | SyntaxKind::Ampersand | SyntaxKind::Ellipsis)
    ) {
        type_reference(parser);
    }
    parser.eat(SyntaxKind::Ampersand);
    parser.eat(SyntaxKind::Ellipsis);
    parser.expect(SyntaxKind::Variable);
    if parser.eat(SyntaxKind::Equals) {
        expression(parser);
    }
    marker.complete(parser, SyntaxKind::Parameter);
}

/// One optionally-nullable named type (`int`, `?\Foo\Bar`, `callable`,
/// `array`, `static`). Union, intersection, and DNF forms arrive with
/// the declarations plan, which replaces this rule.
///
/// The qualified-name tokens are bumped directly, not through `name`:
/// `name` wraps them in their own `Name` node (as `NameExpression`
/// needs, to keep a name a reusable, independently-addressable unit),
/// but a `TypeReference` has no such second consumer, so the tokens sit
/// directly under it.
fn type_reference(parser: &mut Parser) {
    let marker = parser.start();
    parser.eat(SyntaxKind::Question);
    match parser.current() {
        Some(SyntaxKind::Identifier | SyntaxKind::Backslash | SyntaxKind::Namespace) => {
            qualified_type_name(parser);
        }
        Some(kind) if kind.is_keyword() => parser.bump(),
        _ => parser.diagnose_current(ParserDiagnosticKind::Expected(SyntaxKind::Identifier)),
    }
    marker.complete(parser, SyntaxKind::TypeReference);
}

/// `Foo`, `Foo\Bar`, `\Foo`, `namespace\Foo`, bumped directly under the
/// caller's node. Mirrors `name`'s token sequence without its `Name`
/// wrapper.
fn qualified_type_name(parser: &mut Parser) {
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
}

/// `use ( variables )` on a closure. Progress is guaranteed without an
/// explicit committed-position guard: every loop iteration either takes
/// the `error_element` branch (which always bumps) or the
/// ampersand/variable branch, which always consumes at least the
/// ampersand or the variable itself before any refusable sub-parse can
/// run (there is none here; the diagnosed-missing-variable path is
/// zero-width but only reached after the ampersand it followed already
/// advanced the position).
fn closure_use_clause(parser: &mut Parser) {
    let marker = parser.start();
    parser.bump(); // `use`
    if parser.at(SyntaxKind::OpenParenthesis) {
        parser.bump();
        while !parser.at(SyntaxKind::CloseParenthesis) && !parser.at_end() {
            if parser.at(SyntaxKind::Semicolon) || parser.at(SyntaxKind::CloseTag) {
                break;
            }
            if parser.at(SyntaxKind::Ampersand) || parser.at(SyntaxKind::Variable) {
                parser.eat(SyntaxKind::Ampersand);
                if parser.at(SyntaxKind::Variable) {
                    let variable = parser.start();
                    parser.bump();
                    variable.complete(parser, SyntaxKind::VariableReference);
                } else {
                    parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::Variable));
                }
            } else {
                error_element(parser);
                continue;
            }
            expect_list_separator(parser, SyntaxKind::CloseParenthesis);
        }
        parser.expect(SyntaxKind::CloseParenthesis);
    } else {
        parser.diagnose_missing(ParserDiagnosticKind::Expected(SyntaxKind::OpenParenthesis));
    }
    marker.complete(parser, SyntaxKind::ClosureUseClause);
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
