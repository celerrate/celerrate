//! PHP syntax for the Celerrate toolchain: [`lex`] turns decoded source
//! text into a lossless token stream, [`parse`] builds the lossless
//! concrete syntax tree on top of it, plus structured diagnostics, and
//! the [`ast`] module gives typed, `Option`-everywhere access to that
//! tree, generated from `php.ungram`.
//! Nothing here ever fails: degenerate input yields error tokens,
//! `ErrorNode`s, and diagnostics, never a crash.

mod cursor;
mod diagnostic;
mod lexer;
mod parse;
mod parser;
mod syntax_kind;
mod token;
mod tree;

pub mod ast;

pub use diagnostic::{
    ALLOCATED_IDENTIFIERS, LexerDiagnostic, LexerDiagnosticKind, ParserDiagnosticKind,
    SyntaxDiagnostic, SyntaxDiagnosticKind,
};
pub use lexer::lex;
pub use parse::{Parse, parse};
pub use syntax_kind::SyntaxKind;
pub use token::Token;
pub use tree::{PhpLanguage, SyntaxElement, SyntaxNode, SyntaxNodePtr, SyntaxToken};
