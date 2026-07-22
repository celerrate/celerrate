//! Presentation-time did-you-mean (design section 7): computed at
//! render and fix time, for the reported diagnostics only, never
//! inside a memoized query. Nothing computed here is persisted: a
//! candidate goes stale the moment a nearer name appears, and no
//! revalidation record could keep it honest. Inside a phase query the
//! candidate search would also wire the global name set into every
//! file's dependency graph; here it wires into nothing.

/// Optimal string alignment distance (restricted Damerau-Levenshtein)
/// over lowercased characters, abandoned as soon as it provably
/// exceeds `bound`. A transposition of two adjacent characters costs 1
/// edit, not 2: transposition is the dominant typo class (`svae` for
/// `save`, `nmae` for `name`) and plain Levenshtein overcharges it,
/// pushing exactly the typos this feature exists for outside the
/// bound. Lowercasing makes a case-only typo distance 0, which is
/// exactly the fix the case-sensitive spaces (constants, properties,
/// enum cases) want suggested.
// Consumed by enrich (task 2); the allow dies with that task.
#[allow(dead_code)]
fn bounded_distance(written: &str, candidate: &str, bound: usize) -> Option<usize> {
    let written: Vec<char> = written.to_lowercase().chars().collect();
    let candidate: Vec<char> = candidate.to_lowercase().chars().collect();
    if written.len().abs_diff(candidate.len()) > bound {
        return None;
    }
    let mut before_previous: Vec<usize> = (0..=candidate.len()).collect();
    let mut previous: Vec<usize> = (0..=candidate.len()).collect();
    for (row, written_character) in written.iter().enumerate() {
        let mut current: Vec<usize> = Vec::with_capacity(candidate.len() + 1);
        current.push(row + 1);
        for (column, candidate_character) in candidate.iter().enumerate() {
            // The `get` fallbacks are unreachable (the rows are dense
            // by construction); they exist because indexing is denied
            // and a wrong answer here is caught by the tests anyway.
            let substitution = previous.get(column).copied().unwrap_or(usize::MAX - 1)
                + usize::from(written_character != candidate_character);
            let insertion = current.get(column).copied().unwrap_or(usize::MAX - 1) + 1;
            let deletion = previous.get(column + 1).copied().unwrap_or(usize::MAX - 1) + 1;
            let mut best = substitution.min(insertion).min(deletion);
            if row > 0 && column > 0 {
                let previous_written = written.get(row - 1);
                let previous_candidate = candidate.get(column - 1);
                if previous_written == Some(candidate_character)
                    && previous_candidate == Some(written_character)
                {
                    // Adjacent transposition: `..ab` -> `..ba` costs 1,
                    // read off the diagonal two rows up (the `get`
                    // fallback is unreachable for the same reason as
                    // above: the row two back is dense by construction
                    // whenever `row > 0`).
                    let transposition = before_previous
                        .get(column - 1)
                        .copied()
                        .unwrap_or(usize::MAX - 1)
                        + 1;
                    best = best.min(transposition);
                }
            }
            current.push(best);
        }
        if current.iter().min().copied().unwrap_or(0) > bound {
            return None;
        }
        before_previous = previous;
        previous = current;
    }
    previous
        .last()
        .copied()
        .filter(|&distance| distance <= bound)
}

/// The bound the design calls "bounded edit distance": tight for short
/// names (almost anything is within 2 of a 3-letter name), 2 otherwise.
// Consumed by enrich (task 2); the allow dies with that task.
#[allow(dead_code)]
fn distance_bound(name: &str) -> usize {
    if name.chars().count() <= 4 { 1 } else { 2 }
}

/// The ambiguity discipline (design section 7): a unique
/// minimal-distance candidate becomes an applicable suggestion; a tie
/// is listed in a note instead, because bulk `--fix-suggestions` must
/// never apply a guess the engine itself knows is ambiguous.
// Consumed by enrich (task 2); the allow dies with that task.
#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
enum DidYouMean {
    Nothing,
    Unique(String),
    Tie(Vec<String>),
}

// Consumed by enrich (task 2); the allow dies with that task.
#[allow(dead_code)]
fn did_you_mean(written: &str, candidates: Vec<String>) -> DidYouMean {
    let bound = distance_bound(written);
    let mut minimum: Option<usize> = None;
    let mut names: Vec<String> = Vec::new();
    for candidate in candidates {
        let Some(distance) = bounded_distance(written, &candidate, bound) else {
            continue;
        };
        match minimum {
            Some(best) if distance > best => {}
            Some(best) if distance == best => {
                if !names.contains(&candidate) {
                    names.push(candidate);
                }
            }
            _ => {
                minimum = Some(distance);
                names = vec![candidate];
            }
        }
    }
    names.sort();
    match names.len() {
        0 => DidYouMean::Nothing,
        1 => names.pop().map_or(DidYouMean::Nothing, DidYouMean::Unique),
        _ => DidYouMean::Tie(names),
    }
}

/// The last segment of a qualified name: `Lib\Client` -> `Client`.
// Consumed by enrich (task 2); the allow dies with that task.
#[allow(dead_code)]
fn terminal_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    //! `unwrap`/`expect`/indexing are fine here: failing loudly is
    //! what a test should do.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::{DidYouMean, bounded_distance, did_you_mean, distance_bound, terminal_segment};

    #[test]
    fn the_distance_is_optimal_string_alignment_over_lowercased_names() {
        assert_eq!(bounded_distance("svae", "save", 2), Some(1));
        assert_eq!(bounded_distance("nmae", "name", 2), Some(1));
        assert_eq!(bounded_distance("save", "save", 2), Some(0));
        assert_eq!(bounded_distance("php_eol", "PHP_EOL", 2), Some(0));
        assert_eq!(bounded_distance("Activ", "Active", 2), Some(1));
    }

    #[test]
    fn a_distance_beyond_the_bound_is_none_not_a_number() {
        assert_eq!(bounded_distance("draft", "active", 2), None);
        assert_eq!(bounded_distance("a", "abcd", 2), None);
    }

    #[test]
    fn the_bound_is_one_for_short_names_and_two_otherwise() {
        assert_eq!(distance_bound("save"), 1);
        assert_eq!(distance_bound("saved"), 2);
        assert_eq!(distance_bound("é"), 1, "characters, not bytes");
    }

    #[test]
    fn a_unique_minimal_candidate_wins() {
        let outcome = did_you_mean(
            "svae",
            vec!["save".to_owned(), "wave".to_owned(), "unrelated".to_owned()],
        );
        // `svae` -> `save` is 1 (adjacent transposition); `svae` -> `wave`
        // is 3, outside the bound of 1: `save` wins uniquely.
        assert_eq!(outcome, DidYouMean::Unique("save".to_owned()),);
        let outcome = did_you_mean("Activ", vec!["Active".to_owned(), "Passive".to_owned()]);
        assert_eq!(outcome, DidYouMean::Unique("Active".to_owned()));
    }

    #[test]
    fn a_nearer_candidate_replaces_a_farther_one_whatever_the_order() {
        let forward = did_you_mean("sive", vec!["salve".to_owned(), "save".to_owned()]);
        let backward = did_you_mean("sive", vec!["save".to_owned(), "salve".to_owned()]);
        assert_eq!(forward, DidYouMean::Unique("save".to_owned()));
        assert_eq!(forward, backward);
    }

    #[test]
    fn tied_candidates_are_sorted_and_deduplicated() {
        let outcome = did_you_mean(
            "sive",
            vec!["sove".to_owned(), "save".to_owned(), "sove".to_owned()],
        );
        assert_eq!(
            outcome,
            DidYouMean::Tie(vec!["save".to_owned(), "sove".to_owned()]),
        );
    }

    #[test]
    fn no_candidate_in_bound_is_nothing() {
        assert_eq!(
            did_you_mean("svae", vec!["unrelated".to_owned()]),
            DidYouMean::Nothing,
        );
        assert_eq!(did_you_mean("svae", Vec::new()), DidYouMean::Nothing);
    }

    #[test]
    fn the_terminal_segment_is_the_name_after_the_last_backslash() {
        assert_eq!(terminal_segment("Lib\\Client"), "Client");
        assert_eq!(terminal_segment("Client"), "Client");
        assert_eq!(terminal_segment("\\App\\Http\\Kernel"), "Kernel");
    }
}
