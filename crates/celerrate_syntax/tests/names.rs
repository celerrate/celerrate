mod support;

use celerrate_syntax::SyntaxKind::*;
use support::{kinds, texts};

#[test]
fn identifiers_and_keywords() {
    assert_eq!(
        kinds("<?php echo $name;"),
        [OpenTag, Whitespace, Echo, Whitespace, Variable, Semicolon]
    );
}

#[test]
fn keywords_are_case_insensitive_but_keep_their_spelling() {
    assert_eq!(
        texts("<?php ECHO Fn"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (Echo, "ECHO".to_owned()),
            (Whitespace, " ".to_owned()),
            (Fn, "Fn".to_owned()),
        ]
    );
}

#[test]
fn non_keyword_names_are_identifiers() {
    assert_eq!(
        kinds("<?php strlen true parent"),
        [
            OpenTag, Whitespace, Identifier, Whitespace, Identifier, Whitespace, Identifier
        ]
    );
}

#[test]
fn names_accept_underscores_digits_and_non_ascii() {
    assert_eq!(
        kinds("<?php _private2 éléphant"),
        [OpenTag, Whitespace, Identifier, Whitespace, Identifier]
    );
}

#[test]
fn variables_carry_their_dollar_sign() {
    assert_eq!(
        texts("<?php $café"),
        [
            (OpenTag, "<?php".to_owned()),
            (Whitespace, " ".to_owned()),
            (Variable, "$café".to_owned()),
        ]
    );
}

#[test]
fn variable_variables_split_into_dollar_then_variable() {
    assert_eq!(
        kinds("<?php $$name"),
        [OpenTag, Whitespace, Dollar, Variable]
    );
}

#[test]
fn a_lone_dollar_is_its_own_token() {
    assert_eq!(kinds("<?php $ "), [OpenTag, Whitespace, Dollar, Whitespace]);
}

#[test]
fn keywords_are_not_matched_inside_longer_names() {
    assert_eq!(kinds("<?php echoing"), [OpenTag, Whitespace, Identifier]);
}
