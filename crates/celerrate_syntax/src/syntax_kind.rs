/// Declares `SyntaxKind` and derives the raw `u16` conversion from the
/// same variant list, so the enum and the conversion can never drift
/// apart: `ALL` mirrors the declaration order, and declaration order is
/// discriminant order.
macro_rules! syntax_kinds {
    ( $( $(#[$attribute:meta])* $variant:ident, )* ) => {
        /// Every kind of token and node in PHP syntax.
        ///
        /// One vocabulary shared by the whole syntax layer, `#[repr(u16)]`
        /// so the rowan tree stores it directly. Token kinds first, node
        /// kinds after them.
        ///
        /// Keywords each get their own kind, resolved case-insensitively by
        /// the lexer. Semi-reserved uses (`$object->list()`, `const FOR = 1;`,
        /// `enum` as a plain name) are the parser's business: it re-treats
        /// keyword kinds as identifiers where the grammar allows. `true`,
        /// `false`, `null`, `self`, `parent`, and the magic constants are
        /// plain identifiers, resolved semantically.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u16)]
        pub enum SyntaxKind {
            $( $(#[$attribute])* $variant, )*
        }

        impl SyntaxKind {
            /// Every kind, in declaration (and therefore discriminant) order.
            const ALL: &'static [SyntaxKind] = &[ $(SyntaxKind::$variant,)* ];
        }
    };
}

syntax_kinds! {
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
    /// `yield from`, one token as in Zend, interior whitespace included.
    YieldFrom,

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
    /// `|>`, the PHP 8.5 pipe operator.
    PipeGreater,
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

    // Node kinds, appended after every token kind and hand-maintained
    // until the ungrammar code generation of a later plan takes
    // ownership of this list.
    /// The root node: one parsed PHP file.
    SourceFile,
    /// `echo expression, expression;`.
    EchoStatement,
    /// An expression used as a statement, terminator included.
    ExpressionStatement,
    /// A literal expression: integer, float, or single-quoted string.
    Literal,
    /// A `$variable` used as an expression.
    VariableReference,
    /// Recovery wreckage: tokens no grammar rule accepted.
    ErrorNode,
    /// `( expression )`.
    ParenthesizedExpression,
    /// One binary operation: left operand, operator token, right operand.
    /// The operator token distinguishes `+` from `instanceof` from `|>`.
    BinaryExpression,
    /// A prefix operation: `!`, `~`, unary `+`/`-`, `@`, `++`, `--`.
    PrefixExpression,
    /// A postfix operation: `++`, `--`.
    PostfixExpression,
    /// A cast: the single cast token, then the operand.
    CastExpression,
    /// `condition ? middle : third`; the short form `?:` has no middle.
    TernaryExpression,
    /// `target = value` and the compound forms; `= &value` keeps its
    /// ampersand as a token child. Whether the target is assignable is
    /// a semantic judgment.
    AssignmentExpression,
    /// A possibly-qualified name: `Foo`, `Foo\Bar`, `\Foo`, `namespace\Foo`.
    Name,
    /// A name used as an expression: a constant fetch or a callee.
    NameExpression,
    /// `$$name` and `${expression}`.
    DynamicVariableExpression,
    /// `( argument, ... )`, including the lone `...` of a first-class
    /// callable.
    ArgumentList,
    /// One argument: optional `label:`, optional `...`, optional `&`,
    /// then the expression.
    Argument,
    /// A call: the callee expression, then its argument list.
    CallExpression,
    /// `subject->name` and `subject?->name`.
    MemberAccessExpression,
    /// `subject::name`: constants, methods, static properties, `::class`.
    ScopedAccessExpression,
    /// The name after `->`, `?->`, or `::`: identifier, any keyword,
    /// variable, or `{ expression }`.
    MemberName,
    /// `subject[index]`; the index is absent in the push form `$a[]`.
    IndexExpression,
    /// `[ elements ]` or `array( elements )`; also the destructuring
    /// target shape. Empty destructuring slots keep their commas as
    /// direct children.
    ArrayExpression,
    /// One element: optional `...`, optional `&`, expression, then
    /// optionally `=>` (optional `&`) expression.
    ArrayElement,
    /// `list( elements )`, the keyword destructuring form.
    ListExpression,
    /// `"..."` with fragments and interpolations.
    InterpolatedString,
    /// A heredoc or nowdoc, start to end label.
    HeredocExpression,
    /// A backtick string: shell execution.
    ShellExecExpression,
    /// `$name`, `$name->property`, `$name[offset]` inside a string.
    SimpleInterpolation,
    /// `{ expression }` inside a string.
    BraceInterpolation,
    /// `${ ... }` inside a string, the deprecated form.
    DollarBraceInterpolation,
    /// `new` with a class reference and optional constructor arguments.
    NewExpression,
    /// `clone value` or the 8.5 function form `clone(...)`.
    CloneExpression,
    /// `isset( arguments )`.
    IssetExpression,
    /// `empty( argument )`.
    EmptyExpression,
    /// `eval( argument )`.
    EvalExpression,
    /// `exit` / `die`, with an optional argument list since 8.4.
    ExitExpression,
    /// `print operand`.
    PrintExpression,
    /// `throw operand`, an expression since PHP 8.0.
    ThrowExpression,
    /// `yield`, `yield value`, `yield key => value`, `yield from source`.
    YieldExpression,
    /// `include`, `include_once`, `require`, `require_once`; the
    /// keyword token distinguishes them.
    IncludeExpression,
    /// `match ( subject ) { arms }`.
    MatchExpression,
    /// One arm: a condition list (or `default`), `=>`, the body.
    MatchArm,
    /// `function (...) use (...) { ... }`, optionally `static`, with an
    /// optional by-reference `&` and return type.
    ClosureExpression,
    /// `fn (...) => expression`, optionally `static`.
    ArrowFunctionExpression,
    /// `( parameter, ... )`.
    ParameterList,
    /// One parameter: optional type, `&`, `...`, the variable, and an
    /// optional default.
    Parameter,
    /// `use ( variables )` on a closure.
    ClosureUseClause,
    /// `{ statements }`.
    Block,
    /// A lone `;`.
    EmptyStatement,
    /// `return;` or `return expression;`.
    ReturnStatement,
    /// `break;` or `break level;`; level validity is semantic.
    BreakStatement,
    /// `continue;` or `continue level;`; level validity is semantic.
    ContinueStatement,
    /// `global $a, $b;`.
    GlobalStatement,
    /// `static $a = 1, $b;`, the function-static declaration.
    StaticStatement,
    /// One declared static: the variable and its optional initializer.
    StaticVariable,
    /// `unset( targets );`.
    UnsetStatement,
    /// `goto label;`; whether the label exists is semantic.
    GotoStatement,
    /// `label:`, the target of a `goto`.
    LabelStatement,
    /// `if (condition) body`, with optional `ElseIfClause`s and one
    /// optional `ElseClause`, in either classic or alternative syntax.
    IfStatement,
    /// `elseif (condition) body` (or its alternative-syntax form).
    ElseIfClause,
    /// `else body` (or its alternative-syntax form).
    ElseClause,
    /// `while (condition) body`, either syntax.
    WhileStatement,
    /// `do body while (condition);`.
    DoWhileStatement,
    /// `for (initializers; condition; updates) body`, either syntax.
    ForStatement,
    /// One of `for`'s three sections: a possibly-empty comma-separated
    /// expression list, always present as a node so the sections stay
    /// addressable by position.
    ForExpressionList,
    /// `foreach (subject as key => value) body`, either syntax; the
    /// `=>` separates the optional key target from the value target.
    ForeachStatement,
    /// `switch (subject) { cases }`, either syntax.
    SwitchStatement,
    /// One `case expression:` or `default:` section, its statements
    /// included; the body ends where the next section (or the switch)
    /// begins, so an empty body is a fallthrough.
    SwitchCase,
    /// `try block`, then catch clauses and an optional finally.
    TryStatement,
    /// `catch (Type | Type $variable) block`; the variable is optional
    /// since PHP 8.0.
    CatchClause,
    /// `finally block`.
    FinallyClause,
    /// `declare( directives ) body`, either syntax; the body may be a
    /// lone `;` (an empty statement).
    DeclareStatement,
    /// One `name = value` directive; which names and values are legal
    /// is semantic.
    DeclareDirective,
    /// `function name(parameters): type { body }`, the top-level form;
    /// methods arrive with the declarations plan.
    FunctionDeclaration,
    /// One named type: a qualified `Name`, or a keyword type token
    /// (`array`, `callable`, `static`) sitting bare.
    NamedType,
    /// `?type`.
    NullableType,
    /// `A|B|C`, one flat node for the whole chain.
    UnionType,
    /// `A&B&C`, one flat node for the whole chain.
    IntersectionType,
    /// `( type )` inside a type: the DNF grouping form.
    ParenthesizedType,
    /// `const FOO = 1, BAR = 2;`, optionally typed (8.3), at the top
    /// level or as a class member (with modifiers).
    ConstantDeclaration,
    /// One `name = value` element of a constant declaration.
    ConstantElement,
    /// `namespace A\B;` or `namespace A\B { ... }` or `namespace { ... }`.
    NamespaceDeclaration,
    /// `use A\B;` and every import shape: aliases, `function`/`const`
    /// types, clause lists, group imports.
    UseDeclaration,
    /// One imported name: optional per-item `function`/`const` type
    /// (inside groups), the name, an optional group or alias.
    UseClause,
    /// `\{ items }` of a grouped import.
    UseGroup,
    /// `class Name extends B implements C, D { members }`, with
    /// optional `abstract` / `final` / `readonly` modifiers. Anonymous
    /// classes (`new class(...) { ... }`) share this kind and simply
    /// have no name; their constructor arguments sit before the
    /// heritage clauses.
    ClassDeclaration,
    /// `interface Name extends A, B { members }`.
    InterfaceDeclaration,
    /// `trait Name { members }`.
    TraitDeclaration,
    /// `extends` and its comma-separated names.
    ExtendsClause,
    /// `implements` and its comma-separated names.
    ImplementsClause,
    /// `{ members }` of a class-like body.
    MemberList,
    /// `public int $a = 1, $b;`: modifiers, optional type, then the
    /// declarator elements.
    PropertyDeclaration,
    /// One `$name [= initializer]` element; a hooked property carries
    /// its `PropertyHookList` here (a later task of this plan).
    PropertyElement,
    /// `function name(parameters): type { body }` (or `;` for the
    /// abstract and interface forms) as a class member, modifiers
    /// included.
    MethodDeclaration,
    /// `use TraitA, TraitB;` inside a class body, with an optional
    /// adaptation list instead of the semicolon.
    TraitUseClause,
    /// `{ adaptations }` of a trait use.
    TraitAdaptationList,
    /// `A::member insteadof B, C;`.
    TraitPrecedence,
    /// `[A::]member as [visibility] [name];`.
    TraitAlias,
    /// `enum Name: BackingType implements A { cases and members }`.
    EnumDeclaration,
    /// `case Name;` or `case Name = expression;`.
    EnumCase,
}

/// The longest PHP keywords are `include_once` and `require_once`,
/// tied at twelve bytes.
const LONGEST_KEYWORD_LENGTH: usize = 12;

impl SyntaxKind {
    /// The inverse of [`SyntaxKind::into_raw`]. Total and panic-free:
    /// out-of-range values return `None`.
    pub fn from_raw(raw: u16) -> Option<Self> {
        Self::ALL.get(usize::from(raw)).copied()
    }

    /// The `u16` the tree stores; the discriminant.
    pub fn into_raw(self) -> u16 {
        self as u16
    }

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
    /// being contiguous in the declaration, `Abstract` through
    /// `YieldFrom`; the classifier test pins that layout.
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
