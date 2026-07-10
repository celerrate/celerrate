//! Shared helpers for the lexer integration tests. Every lexing goes
//! through the lossless assertion: concatenated token lengths must cover
//! the source exactly.
// Slicing by accumulated token lengths is the point of these helpers; a
// bad slice must fail the test loudly.
#![allow(clippy::indexing_slicing)]

use celerrate_syntax::{LexerDiagnostic, SyntaxKind, Token, lex};

pub fn assert_lossless(source: &str, tokens: &[Token]) {
    let total: usize = tokens.iter().map(|token| usize::from(token.length)).sum();
    assert_eq!(
        total,
        source.len(),
        "token lengths must cover the source exactly: {tokens:?}"
    );
    assert!(
        tokens.iter().all(|token| u32::from(token.length) > 0),
        "no token may be empty: {tokens:?}"
    );
}

pub fn lex_verified(source: &str) -> (Vec<Token>, Vec<LexerDiagnostic>) {
    let (tokens, diagnostics) = lex(source);
    assert_lossless(source, &tokens);
    (tokens, diagnostics)
}

#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn kinds(source: &str) -> Vec<SyntaxKind> {
    lex_verified(source)
        .0
        .iter()
        .map(|token| token.kind)
        .collect()
}

#[allow(dead_code)] // Used by other test binaries; dead_code is analyzed per test crate.
pub fn texts(source: &str) -> Vec<(SyntaxKind, String)> {
    let (tokens, _diagnostics) = lex_verified(source);
    let mut offset = 0usize;
    tokens
        .iter()
        .map(|token| {
            let end = offset + usize::from(token.length);
            let text = source[offset..end].to_owned();
            offset = end;
            (token.kind, text)
        })
        .collect()
}
