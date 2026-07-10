//! PHP lexical analysis for the Celerrate toolchain. This part ships the
//! lexer: [`lex`] turns decoded source text into a lossless token stream
//! (trivia included, nothing discarded) plus structured diagnostics. The
//! parser and syntax tree arrive in the next Foundations part.

mod diagnostic;
mod syntax_kind;
mod token;

pub use diagnostic::{LexerDiagnostic, LexerDiagnosticKind};
pub use syntax_kind::SyntaxKind;
pub use token::Token;
