use celerrate_source::TextEdit;

/// Two edits that cannot coexist in one edit set: their ranges
/// intersect, or both insert at the same offset and their relative
/// order would be arbitrary. `first` precedes `second` in the total
/// edit order. Conflicts are reported, never silently resolved; the
/// application layer decides what to do with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditConflict {
    pub first: TextEdit,
    pub second: TextEdit,
}

/// Finds the first conflicting pair in an edit slice already sorted by
/// the [`TextEdit`] total order. Touching at a boundary is not a
/// conflict: an insertion at the start of a replaced range
/// deterministically lands before the replacement. Sorting groups
/// intersecting ranges next to each other, so adjacent pairs suffice.
pub(crate) fn find_conflict(sorted: &[TextEdit]) -> Option<EditConflict> {
    sorted.windows(2).find_map(|pair| {
        let [first, second] = pair else {
            return None;
        };
        if first.file != second.file {
            return None;
        }
        let intersects = first.range.end() > second.range.start();
        let racing_insertions = first.range == second.range && first.range.is_empty();
        (intersects || racing_insertions).then(|| EditConflict {
            first: first.clone(),
            second: second.clone(),
        })
    })
}

#[cfg(test)]
mod tests {
    //! `unwrap` is fine here: failing loudly is what a test should do.
    #![allow(clippy::unwrap_used)]

    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

    use super::find_conflict;

    fn edit(file: u32, start: u32, end: u32, replacement: &str) -> TextEdit {
        TextEdit {
            file: FileId::new(file),
            range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            replacement: replacement.to_owned(),
        }
    }

    #[test]
    fn disjoint_edits_do_not_conflict() {
        let edits = [edit(0, 0, 2, "a"), edit(0, 5, 9, "b")];
        assert_eq!(find_conflict(&edits), None);
    }

    #[test]
    fn touching_edits_do_not_conflict() {
        let edits = [edit(0, 0, 5, "a"), edit(0, 5, 9, "b")];
        assert_eq!(find_conflict(&edits), None);
    }

    #[test]
    fn an_insertion_at_the_start_of_a_replacement_does_not_conflict() {
        let edits = [edit(0, 5, 5, "inserted"), edit(0, 5, 9, "replaced")];
        assert_eq!(find_conflict(&edits), None);
    }

    #[test]
    fn intersecting_edits_conflict() {
        let edits = [edit(0, 0, 5, "a"), edit(0, 4, 9, "b")];
        let conflict = find_conflict(&edits).unwrap();
        assert_eq!(conflict.first, edit(0, 0, 5, "a"));
        assert_eq!(conflict.second, edit(0, 4, 9, "b"));
    }

    #[test]
    fn a_replacement_containing_an_insertion_point_conflicts() {
        let edits = [edit(0, 0, 9, "a"), edit(0, 4, 4, "b")];
        assert!(find_conflict(&edits).is_some());
    }

    #[test]
    fn identical_edits_conflict() {
        let edits = [edit(0, 3, 5, "a"), edit(0, 3, 5, "a")];
        assert!(find_conflict(&edits).is_some());
    }

    #[test]
    fn two_insertions_at_the_same_offset_conflict() {
        let edits = [edit(0, 5, 5, "a"), edit(0, 5, 5, "b")];
        assert!(find_conflict(&edits).is_some());
    }

    #[test]
    fn same_ranges_in_different_files_do_not_conflict() {
        let edits = [edit(0, 0, 5, "a"), edit(1, 0, 5, "b")];
        assert_eq!(find_conflict(&edits), None);
    }

    #[test]
    fn the_first_conflicting_pair_is_reported() {
        let edits = [edit(0, 0, 5, "a"), edit(0, 4, 6, "b"), edit(0, 5, 9, "c")];
        let conflict = find_conflict(&edits).unwrap();
        assert_eq!(conflict.first, edit(0, 0, 5, "a"));
        assert_eq!(conflict.second, edit(0, 4, 6, "b"));
    }
}
