use core::str::Chars;

use celerrate_source::TextSize;

/// A char cursor over the source with bounded lookahead and token-length
/// accounting. All arithmetic is in bytes; no indexing anywhere, only
/// iterator consumption and `str` prefix operations on [`rest`](Self::rest).
pub(crate) struct Cursor<'source> {
    characters: Chars<'source>,
    /// The unconsumed input as it was when the current token started.
    rest_at_token_start: &'source str,
}

impl<'source> Cursor<'source> {
    pub(crate) fn new(source: &'source str) -> Self {
        Self {
            characters: source.chars(),
            rest_at_token_start: source,
        }
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.characters.clone().next()
    }

    pub(crate) fn peek_second(&self) -> Option<char> {
        let mut lookahead = self.characters.clone();
        lookahead.next();
        lookahead.next()
    }

    /// The unconsumed input. String-based lookahead (`starts_with`,
    /// case-insensitive tag and cast matching) goes through this.
    pub(crate) fn rest(&self) -> &'source str {
        self.characters.as_str()
    }

    pub(crate) fn bump(&mut self) -> Option<char> {
        self.characters.next()
    }

    /// Advances by `count` bytes. Callers compute `count` from
    /// [`rest`](Self::rest), so it always lands on a char boundary; a
    /// defensive fallback consumes everything on an out-of-range count.
    pub(crate) fn bump_bytes(&mut self, count: usize) {
        self.characters = self.rest().get(count..).unwrap_or_default().chars();
    }

    pub(crate) fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.characters.next();
            true
        } else {
            false
        }
    }

    pub(crate) fn eat_while(&mut self, predicate: impl Fn(char) -> bool) {
        while let Some(character) = self.peek() {
            if !predicate(character) {
                break;
            }
            self.characters.next();
        }
    }

    pub(crate) fn is_at_end(&self) -> bool {
        self.rest().is_empty()
    }

    fn pending_byte_length(&self) -> usize {
        self.rest_at_token_start.len() - self.rest().len()
    }

    /// The text consumed since the current token started.
    pub(crate) fn pending_text(&self) -> &'source str {
        self.rest_at_token_start
            .get(..self.pending_byte_length())
            .unwrap_or_default()
    }

    /// Byte length consumed so far in the current token, without
    /// finishing it. Lets diagnostics point inside a token being built.
    pub(crate) fn pending_length(&self) -> TextSize {
        u32::try_from(self.pending_byte_length())
            .map(TextSize::from)
            .unwrap_or_else(|_| TextSize::from(u32::MAX))
    }

    /// Finishes the current token: returns its byte length and starts the
    /// next one. Inputs are within the 4 GiB `TextSize` cap
    /// (`SourceText` guarantees it); the conversion saturates defensively
    /// rather than failing.
    pub(crate) fn take_length(&mut self) -> TextSize {
        let length = self.pending_length();
        self.rest_at_token_start = self.rest();
        length
    }
}

#[cfg(test)]
mod tests {
    //! `expect` is fine here: failing loudly is what a test should do.
    #![allow(clippy::expect_used)]

    use super::Cursor;

    #[test]
    fn peeks_without_consuming() {
        let cursor = Cursor::new("ab");
        assert_eq!(cursor.peek(), Some('a'));
        assert_eq!(cursor.peek_second(), Some('b'));
        assert_eq!(cursor.rest(), "ab");
    }

    #[test]
    fn bumps_consume_one_character() {
        let mut cursor = Cursor::new("héllo");
        assert_eq!(cursor.bump(), Some('h'));
        assert_eq!(cursor.bump(), Some('é'));
        assert_eq!(cursor.rest(), "llo");
        assert!(!cursor.is_at_end());
    }

    #[test]
    fn take_length_counts_bytes_and_resets() {
        let mut cursor = Cursor::new("é1é2");
        cursor.bump();
        cursor.bump();
        assert_eq!(cursor.pending_text(), "é1");
        assert_eq!(u32::from(cursor.take_length()), 3);
        cursor.bump();
        assert_eq!(cursor.pending_text(), "é");
        assert_eq!(u32::from(cursor.take_length()), 2);
    }

    #[test]
    fn eat_consumes_only_the_expected_character() {
        let mut cursor = Cursor::new("ab");
        assert!(!cursor.eat('b'));
        assert!(cursor.eat('a'));
        assert_eq!(cursor.rest(), "b");
    }

    #[test]
    fn eat_while_stops_at_the_first_rejection() {
        let mut cursor = Cursor::new("aaab");
        cursor.eat_while(|character| character == 'a');
        assert_eq!(cursor.rest(), "b");
        cursor.eat_while(|character| character == 'a');
        assert_eq!(cursor.rest(), "b");
    }

    #[test]
    fn bump_bytes_advances_by_byte_count() {
        let mut cursor = Cursor::new("<?php echo");
        cursor.bump_bytes(5);
        assert_eq!(cursor.pending_text(), "<?php");
        assert_eq!(cursor.rest(), " echo");
    }

    #[test]
    fn bump_bytes_with_an_out_of_range_count_consumes_everything() {
        let mut cursor = Cursor::new("abc");
        cursor.bump_bytes(10);
        assert!(cursor.is_at_end());
        assert_eq!(cursor.rest(), "");
    }

    #[test]
    fn is_at_end_becomes_true_after_full_consumption() {
        let mut cursor = Cursor::new("ab");
        assert!(!cursor.is_at_end());
        cursor.bump();
        cursor.bump();
        assert!(cursor.is_at_end());
    }

    #[test]
    fn end_of_input_is_stable() {
        let mut cursor = Cursor::new("");
        assert!(cursor.is_at_end());
        assert_eq!(cursor.bump(), None);
        assert_eq!(cursor.peek(), None);
        assert_eq!(u32::from(cursor.take_length()), 0);
    }
}
