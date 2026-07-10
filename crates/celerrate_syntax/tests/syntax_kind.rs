use celerrate_syntax::SyntaxKind;

#[test]
fn keywords_resolve_case_insensitively() {
    assert_eq!(SyntaxKind::from_keyword("echo"), Some(SyntaxKind::Echo));
    assert_eq!(SyntaxKind::from_keyword("Echo"), Some(SyntaxKind::Echo));
    assert_eq!(SyntaxKind::from_keyword("ECHO"), Some(SyntaxKind::Echo));
    assert_eq!(
        SyntaxKind::from_keyword("include_once"),
        Some(SyntaxKind::IncludeOnce)
    );
    assert_eq!(
        SyntaxKind::from_keyword("readonly"),
        Some(SyntaxKind::Readonly)
    );
}

#[test]
fn die_is_an_alias_of_exit() {
    assert_eq!(SyntaxKind::from_keyword("exit"), Some(SyntaxKind::Exit));
    assert_eq!(SyntaxKind::from_keyword("die"), Some(SyntaxKind::Exit));
    assert_eq!(SyntaxKind::from_keyword("DIE"), Some(SyntaxKind::Exit));
}

#[test]
fn non_keywords_do_not_resolve() {
    assert_eq!(SyntaxKind::from_keyword("echoes"), None);
    assert_eq!(SyntaxKind::from_keyword("true"), None);
    assert_eq!(SyntaxKind::from_keyword("self"), None);
    assert_eq!(SyntaxKind::from_keyword(""), None);
    assert_eq!(SyntaxKind::from_keyword("très_long_identifiant"), None);
}

#[test]
fn trivia_kinds_are_classified() {
    assert!(SyntaxKind::Whitespace.is_trivia());
    assert!(SyntaxKind::LineComment.is_trivia());
    assert!(SyntaxKind::BlockComment.is_trivia());
    assert!(SyntaxKind::DocComment.is_trivia());
    assert!(SyntaxKind::Shebang.is_trivia());
    assert!(!SyntaxKind::Identifier.is_trivia());
    assert!(!SyntaxKind::InlineHtml.is_trivia());
    assert!(!SyntaxKind::Error.is_trivia());
}

#[test]
fn raw_conversion_roundtrips_every_kind() {
    let mut raw = 0u16;
    while let Some(kind) = SyntaxKind::from_raw(raw) {
        assert_eq!(
            kind.into_raw(),
            raw,
            "discriminant order must match ALL order"
        );
        raw += 1;
    }
    assert!(raw > 0, "at least one kind exists");
    assert_eq!(SyntaxKind::from_raw(raw), None);
    assert_eq!(SyntaxKind::from_raw(u16::MAX), None);
}

#[test]
fn node_kinds_exist_and_are_not_trivia() {
    for kind in [
        SyntaxKind::SourceFile,
        SyntaxKind::EchoStatement,
        SyntaxKind::ExpressionStatement,
        SyntaxKind::Literal,
        SyntaxKind::VariableReference,
        SyntaxKind::ErrorNode,
    ] {
        assert!(!kind.is_trivia());
    }
}

#[test]
fn node_kinds_come_after_token_kinds() {
    assert!(SyntaxKind::SourceFile.into_raw() > SyntaxKind::Error.into_raw());
}
