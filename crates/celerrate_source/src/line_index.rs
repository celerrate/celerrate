use text_size::TextSize;

/// A zero-based line/column position. `col` is a byte offset within the
/// line, not a character count: multi-byte UTF-8 characters advance it by
/// their byte length. Rendering layers convert to user-facing columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// Maps byte offsets to line/column positions and back for one text.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<TextSize>,
    len: TextSize,
}

impl LineIndex {
    /// Builds the index in one pass over the text.
    ///
    /// # Panics
    ///
    /// Panics if `text` is larger than 4 GiB, the maximum size `TextSize`
    /// can represent. Rejecting oversized inputs before indexing is the
    /// responsibility of source-file loading (see the crate documentation).
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![TextSize::from(0)];
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter(|&(_, byte)| byte == b'\n')
                .map(|(position, _)| TextSize::from(position as u32 + 1)),
        );
        Self {
            line_starts,
            len: TextSize::of(text),
        }
    }

    /// Maps a byte offset to its line/column position. Offsets are expected
    /// to lie within the indexed text (`0..=len`); the end-of-text offset
    /// maps to the position just past the last character.
    pub fn line_col(&self, offset: TextSize) -> LineCol {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts.get(line).copied().unwrap_or_default();
        LineCol {
            line: line as u32,
            col: u32::from(offset) - u32::from(line_start),
        }
    }

    /// Maps a line/column position back to a byte offset. Returns `None`
    /// when the line does not exist, the column runs past the end of the
    /// line, or the position is not representable. The column one past the
    /// line's last byte is accepted: on interior lines it is the next
    /// line's start (which `line_col` reports as the next line's column
    /// zero), on the last line it is the end of text.
    pub fn offset(&self, line_col: LineCol) -> Option<TextSize> {
        let line = usize::try_from(line_col.line).ok()?;
        let line_start = self.line_starts.get(line).copied()?;
        let candidate = line_start.checked_add(TextSize::from(line_col.col))?;
        let line_end = self.line_starts.get(line + 1).copied().unwrap_or(self.len);
        (candidate <= line_end).then_some(candidate)
    }
}
