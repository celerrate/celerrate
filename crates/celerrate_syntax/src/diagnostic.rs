use celerrate_source::TextRange;

use crate::syntax_kind::SyntaxKind;

/// What went wrong, structurally. Rendering into messages is an upper
/// layer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerDiagnosticKind {
    /// A character no lexing rule accepts; the matching token is `Error`.
    UnexpectedCharacter,
    /// `/*` without `*/`; the comment token runs to the end of input.
    UnterminatedBlockComment,
    /// A quoted or backtick string still open at the end of input.
    UnterminatedString,
    /// A heredoc or nowdoc whose closing label never appears.
    UnterminatedHeredoc,
    /// `{$` or `${` without its closing brace.
    UnterminatedInterpolation,
}

/// A lexer diagnostic: a structured kind and the range it points at.
///
/// Diagnostics travel beside the token stream, never instead of it: the
/// stream stays complete and lossless even on degenerate input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexerDiagnostic {
    pub kind: LexerDiagnosticKind,
    pub range: TextRange,
}

/// What the parser expected or could not place, structurally. Rendering
/// into messages is an upper layer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserDiagnosticKind {
    /// An expression position holds no expression.
    ExpectedExpression,
    /// A statement misses its terminator (`;`, or `?>` / end of input
    /// only where PHP itself allows the omission).
    ExpectedSemicolon,
    /// A specific token is missing; the range is zero-width at the spot
    /// where it belongs.
    Expected(SyntaxKind),
    /// A token no grammar rule accepts; wrapped in an `ErrorNode`.
    UnexpectedToken,
    /// Expressions nest deeper than the parser's recursion budget; the
    /// innermost expression is missing from the tree, the tokens are
    /// preserved through recovery.
    NestingTooDeep,
    /// A non-associative operator chained at the same level, which Zend
    /// rejects (`1 < 2 < 3`, unparenthesized ternary chains, double
    /// `instanceof`). Parsed left-associatively anyway.
    NonAssociativeOperator,
    /// The parser stopped consuming input: an internal grammar loop made
    /// no progress within its step budget. The remaining tokens are
    /// preserved in an `ErrorNode`. Reaching this is a parser bug, kept
    /// survivable by design.
    NoProgress,
    /// `->`, `?->`, or `::` with nothing usable after it.
    ExpectedMemberName,
    /// A control-flow body position holds no statement (`if ($x)` at
    /// end of input, or directly against a closing keyword).
    ExpectedStatement,
    /// A type position holds no type (`function f(): {}`).
    ExpectedType,
    /// A position that requires a declaration holds none (attribute
    /// groups in front of a non-declaration, modifiers with nothing to
    /// modify).
    ExpectedDeclaration,
}

/// A parser diagnostic: a structured kind and the range it points at.
/// Zero-width ranges mark something missing at that offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParserDiagnostic {
    pub kind: ParserDiagnosticKind,
    pub range: TextRange,
}

/// One diagnostic from the syntax layer, wherever it arose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxDiagnosticKind {
    Lexer(LexerDiagnosticKind),
    Parser(ParserDiagnosticKind),
}

/// A syntax diagnostic: lexer and parser findings merged into one
/// stream, in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxDiagnostic {
    pub kind: SyntaxDiagnosticKind,
    pub range: TextRange,
}
