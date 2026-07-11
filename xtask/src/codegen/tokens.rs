//! The token kinds of `SyntaxKind`, as data: the generator emits the
//! enum's token section from this table (order here is discriminant
//! order, and the keyword block must stay contiguous for
//! `SyntaxKind::is_keyword`), and resolves the token spellings
//! `php.ungram` uses against it.

/// One token kind: its `SyntaxKind` variant name, the spelling
/// `php.ungram` uses for it (`None` for kinds that never appear in a
/// grammar rule, like trivia and tags), and its doc lines.
#[derive(Debug, Clone, Copy)]
pub struct TokenKindDefinition {
    pub variant: &'static str,
    pub ungrammar_name: Option<&'static str>,
    pub documentation: &'static [&'static str],
}

const fn token(
    variant: &'static str,
    ungrammar_name: Option<&'static str>,
    documentation: &'static [&'static str],
) -> TokenKindDefinition {
    TokenKindDefinition {
        variant,
        ungrammar_name,
        documentation,
    }
}

/// The spelling of a `php.ungram` token, resolved to its variant name.
pub fn resolve_ungrammar_token(spelling: &str) -> Option<&'static str> {
    TOKEN_KINDS
        .iter()
        .find(|definition| definition.ungrammar_name == Some(spelling))
        .map(|definition| definition.variant)
}

pub const TOKEN_KINDS: &[TokenKindDefinition] = &[
    // Trivia.
    token("Whitespace", None, &[]),
    token(
        "LineComment",
        None,
        &["`//` and `#` comments, up to the end of the line or a `?>`."],
    ),
    token("BlockComment", None, &["`/* ... */` comments."]),
    token(
        "DocComment",
        None,
        &["`/** ... */` docblocks, a distinct kind: the type engine reads them."],
    ),
    token("Shebang", None, &["A `#!` first line."]),
    // Tags and inline HTML.
    token("OpenTag", None, &["`<?php`."]),
    token("OpenTagEcho", None, &["`<?=`."]),
    token(
        "ShortOpenTag",
        None,
        &["`<?`, lexed unconditionally; availability is a semantic judgment."],
    ),
    token(
        "CloseTag",
        None,
        &["`?>`, plus the single newline PHP swallows after it, if present."],
    ),
    token("InlineHtml", None, &["Everything outside PHP tags."]),
    // Names.
    token("Identifier", Some("identifier"), &[]),
    token("Variable", Some("variable"), &["`$name`."]),
    // Literals and string structure.
    token("IntegerLiteral", Some("integer_literal"), &[]),
    token("FloatLiteral", Some("float_literal"), &[]),
    token(
        "SingleQuotedString",
        Some("single_quoted_string"),
        &["A whole `'...'` (or `b'...'`) string, quotes included."],
    ),
    token(
        "StringFragment",
        Some("string_fragment"),
        &["A literal run inside an interpolated string, heredoc, or backtick."],
    ),
    token(
        "DoubleQuote",
        Some("\""),
        &["A `\"` delimiter (or the opening `b\"`)."],
    ),
    token("Backtick", Some("`"), &["A `` ` `` delimiter."]),
    token(
        "HeredocStart",
        Some("heredoc_start"),
        &["`<<<LABEL` (or quoted label), trailing newline included."],
    ),
    token(
        "HeredocEnd",
        Some("heredoc_end"),
        &["The closing label of a heredoc or nowdoc, indentation included."],
    ),
    token(
        "DollarOpenBrace",
        Some("${"),
        &["`${` opening the deprecated interpolation form."],
    ),
    // Keywords.
    token("Abstract", Some("abstract"), &[]),
    token("And", Some("and"), &[]),
    token("Array", Some("array"), &[]),
    token("As", Some("as"), &[]),
    token("Break", Some("break"), &[]),
    token("Callable", Some("callable"), &[]),
    token("Case", Some("case"), &[]),
    token("Catch", Some("catch"), &[]),
    token("Class", Some("class"), &[]),
    token("Clone", Some("clone"), &[]),
    token("Const", Some("const"), &[]),
    token("Continue", Some("continue"), &[]),
    token("Declare", Some("declare"), &[]),
    token("Default", Some("default"), &[]),
    token("Do", Some("do"), &[]),
    token("Echo", Some("echo"), &[]),
    token("Else", Some("else"), &[]),
    token("ElseIf", Some("elseif"), &[]),
    token("Empty", Some("empty"), &[]),
    token("EndDeclare", Some("enddeclare"), &[]),
    token("EndFor", Some("endfor"), &[]),
    token("EndForeach", Some("endforeach"), &[]),
    token("EndIf", Some("endif"), &[]),
    token("EndSwitch", Some("endswitch"), &[]),
    token("EndWhile", Some("endwhile"), &[]),
    token("Enum", Some("enum"), &[]),
    token("Eval", Some("eval"), &[]),
    token("Exit", Some("exit"), &["`exit` and its alias `die`."]),
    token("Extends", Some("extends"), &[]),
    token("Final", Some("final"), &[]),
    token("Finally", Some("finally"), &[]),
    token("Fn", Some("fn"), &[]),
    token("For", Some("for"), &[]),
    token("Foreach", Some("foreach"), &[]),
    token("Function", Some("function"), &[]),
    token("Global", Some("global"), &[]),
    token("Goto", Some("goto"), &[]),
    token("If", Some("if"), &[]),
    token("Implements", Some("implements"), &[]),
    token("Include", Some("include"), &[]),
    token("IncludeOnce", Some("include_once"), &[]),
    token("InstanceOf", Some("instanceof"), &[]),
    token("InsteadOf", Some("insteadof"), &[]),
    token("Interface", Some("interface"), &[]),
    token("Isset", Some("isset"), &[]),
    token("List", Some("list"), &[]),
    token("Match", Some("match"), &[]),
    token("Namespace", Some("namespace"), &[]),
    token("New", Some("new"), &[]),
    token("Or", Some("or"), &[]),
    token("Print", Some("print"), &[]),
    token("Private", Some("private"), &[]),
    token("Protected", Some("protected"), &[]),
    token("Public", Some("public"), &[]),
    token("Readonly", Some("readonly"), &[]),
    token("Require", Some("require"), &[]),
    token("RequireOnce", Some("require_once"), &[]),
    token("Return", Some("return"), &[]),
    token("Static", Some("static"), &[]),
    token("Switch", Some("switch"), &[]),
    token("Throw", Some("throw"), &[]),
    token("Trait", Some("trait"), &[]),
    token("Try", Some("try"), &[]),
    token("Unset", Some("unset"), &[]),
    token("Use", Some("use"), &[]),
    token("Var", Some("var"), &[]),
    token("While", Some("while"), &[]),
    token("Xor", Some("xor"), &[]),
    token("Yield", Some("yield"), &[]),
    token(
        "YieldFrom",
        Some("yield from"),
        &["`yield from`, one token as in Zend, interior whitespace included."],
    ),
    // Casts (single tokens, inner whitespace included).
    token("IntCast", Some("(int)"), &[]),
    token("BoolCast", Some("(bool)"), &[]),
    token("FloatCast", Some("(float)"), &[]),
    token("StringCast", Some("(string)"), &[]),
    token("BinaryCast", Some("(binary)"), &[]),
    token("ArrayCast", Some("(array)"), &[]),
    token("ObjectCast", Some("(object)"), &[]),
    // Operators and punctuation.
    token("Plus", Some("+"), &[]),
    token("Minus", Some("-"), &[]),
    token("Star", Some("*"), &[]),
    token("Slash", Some("/"), &[]),
    token("Percent", Some("%"), &[]),
    token("StarStar", Some("**"), &[]),
    token("Equals", Some("="), &[]),
    token("PlusEquals", Some("+="), &[]),
    token("MinusEquals", Some("-="), &[]),
    token("StarEquals", Some("*="), &[]),
    token("SlashEquals", Some("/="), &[]),
    token("DotEquals", Some(".="), &[]),
    token("PercentEquals", Some("%="), &[]),
    token("StarStarEquals", Some("**="), &[]),
    token("AmpersandEquals", Some("&="), &[]),
    token("PipeEquals", Some("|="), &[]),
    token("CaretEquals", Some("^="), &[]),
    token("LessLessEquals", Some("<<="), &[]),
    token("GreaterGreaterEquals", Some(">>="), &[]),
    token("QuestionQuestionEquals", Some("??="), &[]),
    token("EqualsEquals", Some("=="), &[]),
    token("EqualsEqualsEquals", Some("==="), &[]),
    token("BangEquals", Some("!="), &["`!=` and its alias `<>`."]),
    token("BangEqualsEquals", Some("!=="), &[]),
    token("Less", Some("<"), &[]),
    token("Greater", Some(">"), &[]),
    token("LessEquals", Some("<="), &[]),
    token("GreaterEquals", Some(">="), &[]),
    token("Spaceship", Some("<=>"), &["`<=>`."]),
    token("PlusPlus", Some("++"), &[]),
    token("MinusMinus", Some("--"), &[]),
    token("LessLess", Some("<<"), &[]),
    token("GreaterGreater", Some(">>"), &[]),
    token("Dot", Some("."), &[]),
    token("Bang", Some("!"), &[]),
    token("AmpersandAmpersand", Some("&&"), &[]),
    token("PipePipe", Some("||"), &[]),
    token("QuestionQuestion", Some("??"), &[]),
    token("Question", Some("?"), &[]),
    token("Colon", Some(":"), &[]),
    token("ColonColon", Some("::"), &[]),
    token("Semicolon", Some(";"), &[]),
    token("Comma", Some(","), &[]),
    token("Ampersand", Some("&"), &[]),
    token("Pipe", Some("|"), &[]),
    token(
        "PipeGreater",
        Some("|>"),
        &["`|>`, the PHP 8.5 pipe operator."],
    ),
    token("Caret", Some("^"), &[]),
    token("Tilde", Some("~"), &[]),
    token("At", Some("@"), &[]),
    token("Dollar", Some("$"), &[]),
    token("Backslash", Some("backslash"), &[]),
    token("Arrow", Some("->"), &["`->`."]),
    token("NullsafeArrow", Some("?->"), &["`?->`."]),
    token("FatArrow", Some("=>"), &["`=>`."]),
    token("Ellipsis", Some("..."), &["`...`."]),
    token("OpenParenthesis", Some("("), &[]),
    token("CloseParenthesis", Some(")"), &[]),
    token("OpenBracket", Some("["), &[]),
    token("CloseBracket", Some("]"), &[]),
    token("OpenBrace", Some("{"), &[]),
    token("CloseBrace", Some("}"), &[]),
    token(
        "AttributeOpen",
        Some("#["),
        &["`#[`, distinct from the `#` line comment."],
    ),
    token("Error", None, &["A character no rule accepts."]),
];

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::{TOKEN_KINDS, resolve_ungrammar_token};

    #[test]
    fn variant_names_are_unique_and_nonempty() {
        let mut seen = std::collections::HashSet::new();
        for definition in TOKEN_KINDS {
            assert!(!definition.variant.is_empty());
            assert!(
                seen.insert(definition.variant),
                "duplicate variant {}",
                definition.variant
            );
        }
    }

    #[test]
    fn ungrammar_spellings_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for definition in TOKEN_KINDS {
            if let Some(spelling) = definition.ungrammar_name {
                assert!(seen.insert(spelling), "duplicate spelling {spelling}");
            }
        }
    }

    #[test]
    fn spellings_resolve_to_variants() {
        assert_eq!(resolve_ungrammar_token("function"), Some("Function"));
        assert_eq!(resolve_ungrammar_token("("), Some("OpenParenthesis"));
        assert_eq!(resolve_ungrammar_token("yield from"), Some("YieldFrom"));
        assert_eq!(resolve_ungrammar_token("#["), Some("AttributeOpen"));
        assert_eq!(resolve_ungrammar_token("backslash"), Some("Backslash"));
        assert_eq!(resolve_ungrammar_token("identifier"), Some("Identifier"));
        assert_eq!(resolve_ungrammar_token("(int)"), Some("IntCast"));
        assert_eq!(resolve_ungrammar_token("not a token"), None);
        // Trivia and tags never appear in php.ungram.
        assert!(
            TOKEN_KINDS
                .iter()
                .find(|definition| definition.variant == "Whitespace")
                .expect("Whitespace exists")
                .ungrammar_name
                .is_none()
        );
    }

    #[test]
    fn the_keyword_block_is_contiguous_from_abstract_to_yield_from() {
        // `SyntaxKind::is_keyword` relies on this layout; the enum-side
        // test pins it again after generation.
        let start = TOKEN_KINDS
            .iter()
            .position(|definition| definition.variant == "Abstract")
            .expect("Abstract exists");
        let end = TOKEN_KINDS
            .iter()
            .position(|definition| definition.variant == "YieldFrom")
            .expect("YieldFrom exists");
        assert_eq!(end - start + 1, 70, "seventy keyword kinds");
    }
}
