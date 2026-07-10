use celerrate_source::{TextRange, TextSize};

use crate::syntax_kind::SyntaxKind;
use crate::token::Token;

/// The parser's trivia-free view of the token stream. Positions index
/// significant tokens only; ranges stay absolute in the source, so
/// diagnostics point at real text. The builder reconciles with the full
/// stream by consuming one significant token per `Token` event.
pub(crate) struct TokenSource {
    significant: Vec<(SyntaxKind, TextRange)>,
}

impl TokenSource {
    pub(crate) fn new(tokens: &[Token]) -> Self {
        let mut significant = Vec::new();
        let mut offset = TextSize::from(0);
        for token in tokens {
            if !token.kind.is_trivia() {
                significant.push((token.kind, TextRange::at(offset, token.length)));
            }
            offset += token.length;
        }
        Self { significant }
    }

    pub(crate) fn kind(&self, position: usize) -> Option<SyntaxKind> {
        self.significant.get(position).map(|(kind, _)| *kind)
    }

    pub(crate) fn range(&self, position: usize) -> Option<TextRange> {
        self.significant.get(position).map(|(_, range)| *range)
    }

    pub(crate) fn significant_count(&self) -> usize {
        self.significant.len()
    }
}

#[cfg(test)]
mod tests {
    use crate::syntax_kind::SyntaxKind;

    use super::TokenSource;

    #[test]
    fn trivia_are_invisible_and_ranges_accumulate() {
        // "<?php echo 1;": trivia (whitespace) must vanish from the
        // significant view while ranges stay absolute.
        let (tokens, _diagnostics) = crate::lexer::lex("<?php echo 1;");
        let source = TokenSource::new(&tokens);
        assert_eq!(source.significant_count(), 4);
        assert_eq!(source.kind(0), Some(SyntaxKind::OpenTag));
        assert_eq!(source.kind(1), Some(SyntaxKind::Echo));
        assert_eq!(source.kind(2), Some(SyntaxKind::IntegerLiteral));
        assert_eq!(source.kind(3), Some(SyntaxKind::Semicolon));
        assert_eq!(source.kind(4), None);
        let range = source.range(2).unwrap_or_default();
        assert_eq!(u32::from(range.start()), 11);
        assert_eq!(u32::from(range.end()), 12);
    }
}
