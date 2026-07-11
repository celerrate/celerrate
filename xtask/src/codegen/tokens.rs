//! The token kinds of `SyntaxKind`, as data: the generator emits the
//! enum's token section from this table (order here is discriminant
//! order, and the keyword block must stay contiguous for
//! `SyntaxKind::is_keyword`), and resolves the token spellings
//! `php.ungram` uses against it.

/// One token kind: its `SyntaxKind` variant name, the spelling
/// `php.ungram` uses for it (`None` for kinds that never appear in a
/// grammar rule, like trivia and tags), how it reads inside a diagnostic
/// message when it has no fixed source spelling, and its doc lines.
#[derive(Debug, Clone, Copy)]
pub struct TokenKindDefinition {
    pub variant: &'static str,
    pub ungrammar_name: Option<&'static str>,
    pub description: Option<&'static str>,
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
        description: None,
        documentation,
    }
}

/// A token whose `ungrammar_name` is a symbolic name rather than source
/// text, or which never appears in the grammar at all: it needs an
/// explicit reading.
const fn described(
    variant: &'static str,
    ungrammar_name: Option<&'static str>,
    description: &'static str,
    documentation: &'static [&'static str],
) -> TokenKindDefinition {
    TokenKindDefinition {
        variant,
        ungrammar_name,
        description: Some(description),
        documentation,
    }
}

impl TokenKindDefinition {
    /// How this token reads inside a diagnostic message: an explicit
    /// phrase when it has one, its source spelling in backticks
    /// otherwise. `None` means the table is incomplete, which fails
    /// codegen: a token that describes as nothing would fall back to a
    /// Rust variant name, and no user may ever read one.
    pub fn describe(&self) -> Option<String> {
        match (self.description, self.ungrammar_name) {
            (Some(phrase), _) => Some(phrase.to_owned()),
            (None, Some(spelling)) => Some(format!("`{spelling}`")),
            (None, None) => None,
        }
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
    described("Whitespace", None, "whitespace", &[]),
    described(
        "LineComment",
        None,
        "a line comment",
        &["`//` and `#` comments, up to the end of the line or a `?>`."],
    ),
    described(
        "BlockComment",
        None,
        "a block comment",
        &["`/* ... */` comments."],
    ),
    described(
        "DocComment",
        None,
        "a docblock",
        &["`/** ... */` docblocks, a distinct kind: the type engine reads them."],
    ),
    described("Shebang", None, "a shebang line", &["A `#!` first line."]),
    // Tags and inline HTML.
    described("OpenTag", None, "`<?php`", &["`<?php`."]),
    described("OpenTagEcho", None, "`<?=`", &["`<?=`."]),
    described(
        "ShortOpenTag",
        None,
        "`<?`",
        &["`<?`, lexed unconditionally; availability is a semantic judgment."],
    ),
    described(
        "CloseTag",
        None,
        "`?>`",
        &["`?>`, plus the single newline PHP swallows after it, if present."],
    ),
    described(
        "InlineHtml",
        None,
        "inline HTML",
        &["Everything outside PHP tags."],
    ),
    // Names.
    described("Identifier", Some("identifier"), "a name", &[]),
    described("Variable", Some("variable"), "a variable", &["`$name`."]),
    // Literals and string structure.
    described(
        "IntegerLiteral",
        Some("integer_literal"),
        "an integer literal",
        &[],
    ),
    described(
        "FloatLiteral",
        Some("float_literal"),
        "a float literal",
        &[],
    ),
    described(
        "SingleQuotedString",
        Some("single_quoted_string"),
        "a single-quoted string",
        &["A whole `'...'` (or `b'...'`) string, quotes included."],
    ),
    described(
        "StringFragment",
        Some("string_fragment"),
        "a string fragment",
        &["A literal run inside an interpolated string, heredoc, or backtick."],
    ),
    token(
        "DoubleQuote",
        Some("\""),
        &["A `\"` delimiter (or the opening `b\"`)."],
    ),
    token("Backtick", Some("`"), &["A `` ` `` delimiter."]),
    described(
        "HeredocStart",
        Some("heredoc_start"),
        "a heredoc opener",
        &["`<<<LABEL` (or quoted label), trailing newline included."],
    ),
    described(
        "HeredocEnd",
        Some("heredoc_end"),
        "a heredoc closer",
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
    described("Backslash", Some("backslash"), "`\\`", &[]),
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
    described(
        "Error",
        None,
        "an unrecognized character",
        &["A character no rule accepts."],
    ),
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

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

    #[test]
    fn every_token_describes_itself_without_leaking_its_variant_name() {
        for definition in TOKEN_KINDS {
            let described = definition
                .describe()
                .unwrap_or_else(|| panic!("{} has no description", definition.variant));
            assert!(
                !described.is_empty(),
                "{} describes as nothing",
                definition.variant
            );
            assert_ne!(
                described, definition.variant,
                "{} would leak its Rust variant name to a user",
                definition.variant,
            );
        }
    }

    #[test]
    fn spellings_are_quoted_and_phrases_are_not() {
        let describe = |variant: &str| {
            TOKEN_KINDS
                .iter()
                .find(|definition| definition.variant == variant)
                .and_then(|definition| definition.describe())
                .unwrap()
        };
        assert_eq!(describe("OpenBrace"), "`{`");
        assert_eq!(describe("Semicolon"), "`;`");
        assert_eq!(describe("Class"), "`class`");
        assert_eq!(describe("YieldFrom"), "`yield from`");
        assert_eq!(describe("Identifier"), "a name");
        assert_eq!(describe("Variable"), "a variable");
        assert_eq!(describe("Backslash"), "`\\`");
        assert_eq!(describe("OpenTag"), "`<?php`");
        assert_eq!(describe("Whitespace"), "whitespace");
    }
}
