use celerrate_source::TextSize;

use crate::syntax_kind::SyntaxKind;

/// One lexed token: a kind and a byte length, rust-analyzer style.
///
/// No offset is stored; positions are reconstructed by accumulating
/// lengths, which makes overlaps and gaps structurally impossible. The
/// lossless invariant: concatenating every token's text reproduces the
/// input byte for byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub length: TextSize,
}

impl Token {
    pub fn new(kind: SyntaxKind, length: TextSize) -> Self {
        Self { kind, length }
    }
}
