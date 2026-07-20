use crate::{FileId, TextRange};

/// One finalized textual replacement: `replacement` takes the place of
/// `range` in `file`. The terminal, tree-free form every structured edit
/// compiles down to: suggestions transport it, and the application engine
/// consumes it. Defined at the bottom of the workspace so the diagnostics
/// model, the edit library, and later the formatter and migrations all
/// take it from below. Ordering is total and deterministic so edit sets
/// can be sorted, compared, and checked for overlap byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextEdit {
    pub file: FileId,
    pub range: TextRange,
    pub replacement: String,
}

impl Ord for TextEdit {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (
            self.file,
            self.range.start(),
            self.range.end(),
            &self.replacement,
        )
            .cmp(&(
                other.file,
                other.range.start(),
                other.range.end(),
                &other.replacement,
            ))
    }
}

impl PartialOrd for TextEdit {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use crate::{FileId, TextEdit, TextRange, TextSize};

    fn edit(file: u32, start: u32, end: u32, replacement: &str) -> TextEdit {
        TextEdit {
            file: FileId::new(file),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            replacement: replacement.to_owned(),
        }
    }

    #[test]
    fn edits_order_by_file_then_range_then_replacement() {
        let mut edits = [
            edit(1, 0, 1, "b"),
            edit(0, 5, 9, "a"),
            edit(0, 0, 4, "b"),
            edit(0, 0, 4, "a"),
        ];
        edits.sort();
        assert_eq!(
            edits
                .iter()
                .map(|edit| (edit.file.as_u32(), edit.replacement.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "a"), (0, "b"), (0, "a"), (1, "b")],
        );
    }

    #[test]
    fn equal_edits_compare_equal() {
        assert_eq!(edit(0, 0, 1, "x"), edit(0, 0, 1, "x"));
    }
}
