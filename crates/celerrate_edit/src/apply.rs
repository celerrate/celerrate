use celerrate_source::TextEdit;

use crate::conflict::{EditConflict, find_conflict};

/// Why an edit set could not be applied to a source text. Nothing is
/// ever dropped or resolved silently: the caller decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// Two edits intersect, or race for the same insertion point.
    Conflict(EditConflict),
    /// One application targets one file; this edit belongs to another.
    MultipleFiles { edit: TextEdit },
    /// The edit's range does not fit the source text.
    RangeOutOfBounds {
        edit: TextEdit,
        source_length: usize,
    },
    /// The edit's range splits a multi-byte character.
    RangeNotOnCharacterBoundary { edit: TextEdit },
}

/// Applies a finalized edit set to one file's source text.
///
/// The edits are sorted into the [`TextEdit`] total order first, so
/// the result never depends on input order; conflicts, foreign files,
/// and ill-fitting ranges are errors. The empty set returns the source
/// unchanged.
pub fn apply(source: &str, edits: &[TextEdit]) -> Result<String, ApplyError> {
    let mut sorted = edits.to_vec();
    sorted.sort();
    if let Some(foreign) = sorted
        .first()
        .and_then(|first| sorted.iter().find(|edit| edit.file != first.file))
    {
        return Err(ApplyError::MultipleFiles {
            edit: foreign.clone(),
        });
    }
    if let Some(conflict) = find_conflict(&sorted) {
        return Err(ApplyError::Conflict(conflict));
    }
    for edit in &sorted {
        let start = usize::from(edit.range.start());
        let end = usize::from(edit.range.end());
        if end > source.len() {
            return Err(ApplyError::RangeOutOfBounds {
                edit: edit.clone(),
                source_length: source.len(),
            });
        }
        if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            return Err(ApplyError::RangeNotOnCharacterBoundary { edit: edit.clone() });
        }
    }
    let mut patched = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in &sorted {
        let start = usize::from(edit.range.start());
        // Unreachable fallback: the ranges were just validated as
        // sorted, disjoint, in bounds, and on character boundaries.
        patched.push_str(source.get(cursor..start).unwrap_or(""));
        patched.push_str(&edit.replacement);
        cursor = usize::from(edit.range.end());
    }
    patched.push_str(source.get(cursor..).unwrap_or(""));
    Ok(patched)
}

#[cfg(test)]
mod tests {
    //! `unwrap` is fine here: failing loudly is what a test should do.
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

    use super::{ApplyError, apply};

    fn edit(start: u32, end: u32, replacement: &str) -> TextEdit {
        TextEdit {
            file: FileId::new(0),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            replacement: replacement.to_owned(),
        }
    }

    #[test]
    fn no_edits_return_the_source_unchanged() {
        assert_eq!(apply("<?php echo 1;", &[]).unwrap(), "<?php echo 1;");
    }

    #[test]
    fn a_single_replacement_is_spliced() {
        // "<?php echo 1;" — replace "1" (offsets 11..12) with "2".
        assert_eq!(
            apply("<?php echo 1;", &[edit(11, 12, "2")]).unwrap(),
            "<?php echo 2;",
        );
    }

    #[test]
    fn an_insertion_and_a_deletion_compose() {
        // "abcdef": insert "X" at 2, delete "de" (3..5).
        assert_eq!(
            apply("abcdef", &[edit(2, 2, "X"), edit(3, 5, "")]).unwrap(),
            "abXcf",
        );
    }

    #[test]
    fn the_result_does_not_depend_on_input_order() {
        let forward = apply("abcdef", &[edit(0, 1, "X"), edit(3, 4, "Y")]).unwrap();
        let backward = apply("abcdef", &[edit(3, 4, "Y"), edit(0, 1, "X")]).unwrap();
        assert_eq!(forward, backward);
        assert_eq!(forward, "XbcYef");
    }

    #[test]
    fn an_insertion_at_the_end_of_the_source_is_valid() {
        assert_eq!(apply("abc", &[edit(3, 3, "!")]).unwrap(), "abc!");
    }

    #[test]
    fn intersecting_edits_are_a_conflict() {
        let error = apply("abcdef", &[edit(0, 4, "x"), edit(2, 6, "y")]).unwrap_err();
        assert!(matches!(error, ApplyError::Conflict(_)));
    }

    #[test]
    fn edits_in_different_files_are_rejected() {
        let foreign = TextEdit {
            file: FileId::new(1),
            range: TextRange::new(TextSize::from(0), TextSize::from(1)),
            replacement: "x".to_owned(),
        };
        let error = apply("abcdef", &[edit(3, 4, "y"), foreign.clone()]).unwrap_err();
        assert_eq!(error, ApplyError::MultipleFiles { edit: foreign });
    }

    #[test]
    fn a_range_past_the_end_of_the_source_is_rejected() {
        let error = apply("abc", &[edit(2, 9, "x")]).unwrap_err();
        assert_eq!(
            error,
            ApplyError::RangeOutOfBounds {
                edit: edit(2, 9, "x"),
                source_length: 3,
            },
        );
    }

    #[test]
    fn a_range_splitting_a_multibyte_character_is_rejected() {
        // "héllo": 'é' occupies bytes 1..3; offset 2 splits it.
        let error = apply("héllo", &[edit(2, 4, "x")]).unwrap_err();
        assert_eq!(
            error,
            ApplyError::RangeNotOnCharacterBoundary {
                edit: edit(2, 4, "x"),
            },
        );
    }

    #[test]
    fn multibyte_content_survives_splicing() {
        // "héllo": replace "llo" (bytes 3..6) with "ros".
        assert_eq!(apply("héllo", &[edit(3, 6, "ros")]).unwrap(), "héros");
    }
}
