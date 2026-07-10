/// Every kind of token in PHP source text.
///
/// One vocabulary shared by the whole syntax layer, `#[repr(u16)]` so a
/// future rowan-style tree can store it directly. Token kinds only for
/// now; the parser part appends node kinds after them.
///
/// Keywords each get their own kind, resolved case-insensitively by the
/// lexer. Semi-reserved uses (`$object->list()`, `const FOR = 1;`,
/// `enum` as a plain name) are the parser's business: it re-treats
/// keyword kinds as identifiers where the grammar allows. `true`,
/// `false`, `null`, `self`, `parent`, and the magic constants are plain
/// identifiers, resolved semantically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum SyntaxKind {
    // Trivia.
    Whitespace,
    /// `//` and `#` comments, up to the end of the line or a `?>`.
    LineComment,
    /// `/* ... */` comments.
    BlockComment,
    /// `/** ... */` docblocks, a distinct kind: the type engine reads them.
    DocComment,
    /// A `#!` first line.
    Shebang,

    // Tags and inline HTML.
    /// `<?php`.
    OpenTag,
    /// `<?=`.
    OpenTagEcho,
    /// `<?`, lexed unconditionally; availability is a semantic judgment.
    ShortOpenTag,
    /// `?>`, plus the single newline PHP swallows after it, if present.
    CloseTag,
    /// Everything outside PHP tags.
    InlineHtml,

    // Names.
    Identifier,
    /// `$name`.
    Variable,

    // Literals and string structure.
    IntegerLiteral,
    FloatLiteral,
    /// A whole `'...'` (or `b'...'`) string, quotes included.
    SingleQuotedString,
    /// A literal run inside an interpolated string, heredoc, or backtick.
    StringFragment,
    /// A `"` delimiter (or the opening `b"`).
    DoubleQuote,
    /// A `` ` `` delimiter.
    Backtick,
    /// `<<<LABEL` (or quoted label), trailing newline included.
    HeredocStart,
    /// The closing label of a heredoc or nowdoc, indentation included.
    HeredocEnd,
    /// `${` opening the deprecated interpolation form.
    DollarOpenBrace,

    // Keywords.
    Abstract,
    And,
    Array,
    As,
    Break,
    Callable,
    Case,
    Catch,
    Class,
    Clone,
    Const,
    Continue,
    Declare,
    Default,
    Do,
    Echo,
    Else,
    ElseIf,
    Empty,
    EndDeclare,
    EndFor,
    EndForeach,
    EndIf,
    EndSwitch,
    EndWhile,
    Enum,
    Eval,
    /// `exit` and its alias `die`.
    Exit,
    Extends,
    Final,
    Finally,
    Fn,
    For,
    Foreach,
    Function,
    Global,
    Goto,
    If,
    Implements,
    Include,
    IncludeOnce,
    InstanceOf,
    InsteadOf,
    Interface,
    Isset,
    List,
    Match,
    Namespace,
    New,
    Or,
    Print,
    Private,
    Protected,
    Public,
    Readonly,
    Require,
    RequireOnce,
    Return,
    Static,
    Switch,
    Throw,
    Trait,
    Try,
    Unset,
    Use,
    Var,
    While,
    Xor,
    Yield,

    // Casts (single tokens, inner whitespace included).
    IntCast,
    BoolCast,
    FloatCast,
    StringCast,
    BinaryCast,
    ArrayCast,
    ObjectCast,

    // Operators and punctuation.
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar,
    Equals,
    PlusEquals,
    MinusEquals,
    StarEquals,
    SlashEquals,
    DotEquals,
    PercentEquals,
    StarStarEquals,
    AmpersandEquals,
    PipeEquals,
    CaretEquals,
    LessLessEquals,
    GreaterGreaterEquals,
    QuestionQuestionEquals,
    EqualsEquals,
    EqualsEqualsEquals,
    /// `!=` and its alias `<>`.
    BangEquals,
    BangEqualsEquals,
    Less,
    Greater,
    LessEquals,
    GreaterEquals,
    /// `<=>`.
    Spaceship,
    PlusPlus,
    MinusMinus,
    LessLess,
    GreaterGreater,
    Dot,
    Bang,
    AmpersandAmpersand,
    PipePipe,
    QuestionQuestion,
    Question,
    Colon,
    ColonColon,
    Semicolon,
    Comma,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    At,
    Dollar,
    Backslash,
    /// `->`.
    Arrow,
    /// `?->`.
    NullsafeArrow,
    /// `=>`.
    FatArrow,
    /// `...`.
    Ellipsis,
    OpenParenthesis,
    CloseParenthesis,
    OpenBracket,
    CloseBracket,
    OpenBrace,
    CloseBrace,
    /// `#[`, distinct from the `#` line comment.
    AttributeOpen,

    /// A character no rule accepts.
    Error,
}

/// The longest PHP keywords are `include_once` and `require_once`,
/// tied at twelve bytes.
const LONGEST_KEYWORD_LENGTH: usize = 12;

impl SyntaxKind {
    /// Whether this token carries no syntactic meaning (whitespace,
    /// comments, shebang). Trivia stay in the stream; this classifier is
    /// how upper layers skip them.
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace
                | Self::LineComment
                | Self::BlockComment
                | Self::DocComment
                | Self::Shebang
        )
    }

    /// Resolves a keyword case-insensitively, allocation-free. Returns
    /// `None` when the text is not a PHP keyword.
    pub fn from_keyword(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.is_empty() || bytes.len() > LONGEST_KEYWORD_LENGTH {
            return None;
        }
        let mut buffer = [0u8; LONGEST_KEYWORD_LENGTH];
        let slots = buffer.get_mut(..bytes.len())?;
        for (slot, byte) in slots.iter_mut().zip(bytes) {
            *slot = byte.to_ascii_lowercase();
        }
        let lowered = core::str::from_utf8(buffer.get(..bytes.len())?).ok()?;
        let kind = match lowered {
            "abstract" => Self::Abstract,
            "and" => Self::And,
            "array" => Self::Array,
            "as" => Self::As,
            "break" => Self::Break,
            "callable" => Self::Callable,
            "case" => Self::Case,
            "catch" => Self::Catch,
            "class" => Self::Class,
            "clone" => Self::Clone,
            "const" => Self::Const,
            "continue" => Self::Continue,
            "declare" => Self::Declare,
            "default" => Self::Default,
            "die" => Self::Exit,
            "do" => Self::Do,
            "echo" => Self::Echo,
            "else" => Self::Else,
            "elseif" => Self::ElseIf,
            "empty" => Self::Empty,
            "enddeclare" => Self::EndDeclare,
            "endfor" => Self::EndFor,
            "endforeach" => Self::EndForeach,
            "endif" => Self::EndIf,
            "endswitch" => Self::EndSwitch,
            "endwhile" => Self::EndWhile,
            "enum" => Self::Enum,
            "eval" => Self::Eval,
            "exit" => Self::Exit,
            "extends" => Self::Extends,
            "final" => Self::Final,
            "finally" => Self::Finally,
            "fn" => Self::Fn,
            "for" => Self::For,
            "foreach" => Self::Foreach,
            "function" => Self::Function,
            "global" => Self::Global,
            "goto" => Self::Goto,
            "if" => Self::If,
            "implements" => Self::Implements,
            "include" => Self::Include,
            "include_once" => Self::IncludeOnce,
            "instanceof" => Self::InstanceOf,
            "insteadof" => Self::InsteadOf,
            "interface" => Self::Interface,
            "isset" => Self::Isset,
            "list" => Self::List,
            "match" => Self::Match,
            "namespace" => Self::Namespace,
            "new" => Self::New,
            "or" => Self::Or,
            "print" => Self::Print,
            "private" => Self::Private,
            "protected" => Self::Protected,
            "public" => Self::Public,
            "readonly" => Self::Readonly,
            "require" => Self::Require,
            "require_once" => Self::RequireOnce,
            "return" => Self::Return,
            "static" => Self::Static,
            "switch" => Self::Switch,
            "throw" => Self::Throw,
            "trait" => Self::Trait,
            "try" => Self::Try,
            "unset" => Self::Unset,
            "use" => Self::Use,
            "var" => Self::Var,
            "while" => Self::While,
            "xor" => Self::Xor,
            "yield" => Self::Yield,
            _ => return None,
        };
        Some(kind)
    }
}
