use celerrate_source::TextRange;

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
