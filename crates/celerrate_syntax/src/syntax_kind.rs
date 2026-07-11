//! `SyntaxKind` and its classifiers. The enum itself (token and node
//! variants, the raw `u16` conversion) is generated: token kinds from
//! xtask's token table, node kinds from `php.ungram`. Regenerate with
//! `cargo xtask codegen`; a sourcegen test keeps the committed file
//! fresh. The classifiers below are hand-written because they encode
//! lexer policy (trivia, the keyword table), not grammar shape.

mod generated;

pub use generated::SyntaxKind;

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

    /// Whether this kind is a PHP keyword. Relies on the keyword section
    /// being contiguous in the generated declaration, `Abstract` through
    /// `YieldFrom`; the token table preserves that layout and the
    /// classifier test pins it.
    pub fn is_keyword(self) -> bool {
        (Self::Abstract..=Self::YieldFrom).contains(&self)
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
