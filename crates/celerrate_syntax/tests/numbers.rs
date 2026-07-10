mod support;

use celerrate_syntax::SyntaxKind::*;
use support::{kinds, texts};

fn number_kinds(expression: &str) -> Vec<celerrate_syntax::SyntaxKind> {
    let source = format!("<?php {expression}");
    let mut listing = kinds(&source);
    // Drop the open tag and the following whitespace.
    listing.drain(..2);
    listing
}

#[test]
fn decimal_integers() {
    assert_eq!(number_kinds("0"), [IntegerLiteral]);
    assert_eq!(number_kinds("1234567890"), [IntegerLiteral]);
    assert_eq!(number_kinds("1_000_000"), [IntegerLiteral]);
}

#[test]
fn radix_prefixed_integers() {
    assert_eq!(number_kinds("0xDEAD_beef"), [IntegerLiteral]);
    assert_eq!(number_kinds("0b1010_1010"), [IntegerLiteral]);
    assert_eq!(number_kinds("0o777"), [IntegerLiteral]);
    assert_eq!(number_kinds("0O17"), [IntegerLiteral]);
    assert_eq!(number_kinds("0777"), [IntegerLiteral]);
}

#[test]
fn floats_in_all_shapes() {
    assert_eq!(number_kinds("1.5"), [FloatLiteral]);
    assert_eq!(number_kinds(".5"), [FloatLiteral]);
    assert_eq!(number_kinds("1."), [FloatLiteral]);
    assert_eq!(number_kinds("1e3"), [FloatLiteral]);
    assert_eq!(number_kinds("1E+3"), [FloatLiteral]);
    assert_eq!(number_kinds("1.5e-3"), [FloatLiteral]);
    assert_eq!(number_kinds("1_0.5_0"), [FloatLiteral]);
}

#[test]
fn an_exponent_without_digits_is_not_consumed() {
    // Zend lexes "1e" as an integer then a name; so do we.
    assert_eq!(number_kinds("1e"), [IntegerLiteral, Identifier]);
    assert_eq!(number_kinds("1e+"), [IntegerLiteral, Identifier, Plus]);
}

#[test]
fn number_texts_are_exact() {
    assert_eq!(
        texts("<?php 1.5e3"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (FloatLiteral, "1.5e3".to_owned()),
        ]
    );
}

#[test]
fn hex_prefix_without_digits_is_a_plain_zero() {
    // "0x" alone: integer zero, then the name "x", as in Zend.
    assert_eq!(number_kinds("0x"), [IntegerLiteral, Identifier]);
}

#[test]
fn a_radix_prefix_without_a_valid_digit_is_a_plain_zero() {
    // Zend: the prefix letter joins the following name instead.
    assert_eq!(
        texts("<?php 0xyz"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "0".to_owned()),
            (Identifier, "xyz".to_owned()),
        ]
    );
    assert_eq!(number_kinds("0bz"), [IntegerLiteral, Identifier]);
    assert_eq!(number_kinds("0oz"), [IntegerLiteral, Identifier]);
}

#[test]
fn radix_digit_runs_are_taken_maximally_and_judged_upstairs() {
    // "0b2" stays one literal: digit validity is a semantic judgment.
    assert_eq!(
        texts("<?php 0b2 0o99 0xDEAD_beef 1_000_000"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "0b2".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "0o99".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "0xDEAD_beef".to_owned()),
            (Whitespace, " ".to_owned()),
            (IntegerLiteral, "1_000_000".to_owned()),
        ]
    );
}
