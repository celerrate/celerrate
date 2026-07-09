use celerrate_source::{LineCol, LineIndex, TextSize};

#[test]
fn empty_text_maps_offset_zero_to_origin() {
    let index = LineIndex::new("");
    assert_eq!(
        index.line_col(TextSize::from(0)),
        LineCol { line: 0, col: 0 }
    );
}

#[test]
fn single_line_columns_are_byte_offsets() {
    let index = LineIndex::new("hello");
    assert_eq!(
        index.line_col(TextSize::from(3)),
        LineCol { line: 0, col: 3 }
    );
}

#[test]
fn newline_starts_a_new_line() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(
        index.line_col(TextSize::from(2)),
        LineCol { line: 0, col: 2 }
    );
    assert_eq!(
        index.line_col(TextSize::from(3)),
        LineCol { line: 1, col: 0 }
    );
    assert_eq!(
        index.line_col(TextSize::from(4)),
        LineCol { line: 1, col: 1 }
    );
}

#[test]
fn crlf_newline_keeps_carriage_return_on_its_line() {
    let index = LineIndex::new("ab\r\ncd");
    assert_eq!(
        index.line_col(TextSize::from(2)),
        LineCol { line: 0, col: 2 }
    );
    assert_eq!(
        index.line_col(TextSize::from(4)),
        LineCol { line: 1, col: 0 }
    );
}

#[test]
fn multibyte_characters_advance_columns_by_byte_length() {
    // 'é' is two bytes in UTF-8.
    let index = LineIndex::new("é\nx");
    assert_eq!(
        index.line_col(TextSize::from(2)),
        LineCol { line: 0, col: 2 }
    );
    assert_eq!(
        index.line_col(TextSize::from(3)),
        LineCol { line: 1, col: 0 }
    );
}

#[test]
fn offset_at_end_of_text_is_on_the_last_line() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(
        index.line_col(TextSize::from(5)),
        LineCol { line: 1, col: 2 }
    );
}

#[test]
fn offset_roundtrips_every_char_boundary() {
    let text = "ab\r\ncd\né\nend";
    let index = LineIndex::new(text);
    for (position, _) in text.char_indices() {
        let offset = TextSize::from(position as u32);
        assert_eq!(index.offset(index.line_col(offset)), Some(offset));
    }
}

#[test]
fn offset_accepts_end_of_text() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(
        index.offset(LineCol { line: 1, col: 2 }),
        Some(TextSize::from(5))
    );
}

#[test]
fn offset_rejects_line_out_of_range() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(index.offset(LineCol { line: 2, col: 0 }), None);
}

#[test]
fn offset_rejects_column_past_end_of_line() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(index.offset(LineCol { line: 0, col: 7 }), None);
}

#[test]
fn offset_rejects_column_that_overflows() {
    let index = LineIndex::new("ab\ncd");
    assert_eq!(
        index.offset(LineCol {
            line: 1,
            col: u32::MAX
        }),
        None
    );
}
