mod support;

use celerrate_syntax::SyntaxKind::{self, *};
use support::{kinds, texts};

fn operator_kinds(expression: &str) -> Vec<SyntaxKind> {
    let source = format!("<?php {expression}");
    let mut listing = kinds(&source);
    listing.drain(..2);
    listing.retain(|kind| *kind != Whitespace);
    listing
}

#[test]
fn compound_assignment_operators_lex_longest_first() {
    assert_eq!(operator_kinds("**="), [StarStarEquals]);
    assert_eq!(operator_kinds("** ="), [StarStar, Equals]);
    assert_eq!(operator_kinds("??="), [QuestionQuestionEquals]);
    assert_eq!(operator_kinds("<<="), [LessLessEquals]);
    assert_eq!(operator_kinds(">>="), [GreaterGreaterEquals]);
    assert_eq!(operator_kinds(".="), [DotEquals]);
}

#[test]
fn comparison_operators() {
    assert_eq!(operator_kinds("==="), [EqualsEqualsEquals]);
    assert_eq!(operator_kinds("!=="), [BangEqualsEquals]);
    assert_eq!(operator_kinds("<=>"), [Spaceship]);
    assert_eq!(operator_kinds("<>"), [BangEquals]);
    assert_eq!(operator_kinds("<="), [LessEquals]);
    assert_eq!(operator_kinds("< = >"), [Less, Equals, Greater]);
}

#[test]
fn arrows_and_scope_operators() {
    assert_eq!(operator_kinds("->"), [Arrow]);
    assert_eq!(operator_kinds("?->"), [NullsafeArrow]);
    assert_eq!(operator_kinds("=>"), [FatArrow]);
    assert_eq!(operator_kinds("::"), [ColonColon]);
    assert_eq!(operator_kinds("..."), [Ellipsis]);
    assert_eq!(operator_kinds(".."), [Dot, Dot]);
}

#[test]
fn punctuation_and_delimiters() {
    assert_eq!(
        operator_kinds("( ) [ ] { } , ; @ ~ \\"),
        [
            OpenParenthesis,
            CloseParenthesis,
            OpenBracket,
            CloseBracket,
            OpenBrace,
            CloseBrace,
            Comma,
            Semicolon,
            At,
            Tilde,
            Backslash
        ]
    );
}

#[test]
fn logic_and_bit_operators() {
    assert_eq!(
        operator_kinds("&& & || | ^ !"),
        [AmpersandAmpersand, Ampersand, PipePipe, Pipe, Caret, Bang]
    );
    assert_eq!(
        operator_kinds("?? ? :"),
        [QuestionQuestion, Question, Colon]
    );
    assert_eq!(
        operator_kinds("++ -- + -"),
        [PlusPlus, MinusMinus, Plus, Minus]
    );
    assert_eq!(operator_kinds("<< >>"), [LessLess, GreaterGreater]);
}

#[test]
fn casts_are_single_tokens_with_inner_whitespace() {
    assert_eq!(
        texts("<?php (int)( String )"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntCast, "(int)".to_owned()),
            (StringCast, "( String )".to_owned()),
        ]
    );
}

#[test]
fn all_php_81_cast_forms_resolve() {
    assert_eq!(operator_kinds("(integer)"), [IntCast]);
    assert_eq!(operator_kinds("(bool)"), [BoolCast]);
    assert_eq!(operator_kinds("(boolean)"), [BoolCast]);
    assert_eq!(operator_kinds("(float)"), [FloatCast]);
    assert_eq!(operator_kinds("(double)"), [FloatCast]);
    assert_eq!(operator_kinds("(binary)"), [BinaryCast]);
    assert_eq!(operator_kinds("(array)"), [ArrayCast]);
    assert_eq!(operator_kinds("(object)"), [ObjectCast]);
}

#[test]
fn removed_and_unknown_casts_are_plain_parentheses() {
    assert_eq!(
        operator_kinds("(real)"),
        [OpenParenthesis, Identifier, CloseParenthesis]
    );
    assert_eq!(
        operator_kinds("(unset)"),
        [OpenParenthesis, Unset, CloseParenthesis]
    );
    assert_eq!(
        operator_kinds("(int $x)"),
        [OpenParenthesis, Identifier, Variable, CloseParenthesis]
    );
}

#[test]
fn close_tag_wins_over_question_mark() {
    assert_eq!(kinds("<?php ?>"), [OpenTag, Whitespace, CloseTag]);
}
