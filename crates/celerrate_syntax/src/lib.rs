//! PHP lexical analysis for the Celerrate toolchain. This part ships the
//! lexer: [`lex`] turns decoded source text into a lossless token stream
//! (trivia included, nothing discarded) plus structured diagnostics. The
//! parser and syntax tree arrive in the next Foundations part.

mod cursor;
mod diagnostic;
mod lexer;
mod syntax_kind;
mod token;
mod tree;

pub use diagnostic::{LexerDiagnostic, LexerDiagnosticKind};
pub use lexer::lex;
pub use syntax_kind::SyntaxKind;
pub use token::Token;
pub use tree::{PhpLanguage, SyntaxElement, SyntaxNode, SyntaxToken};
