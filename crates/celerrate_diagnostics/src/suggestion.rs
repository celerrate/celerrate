use celerrate_source::TextEdit;

/// How much a suggestion can be trusted. `Safe` is mass-applicable via
/// `celerrate check --fix` and guaranteed not to change semantics;
/// `NeedsReview` is applied only under `--fix-suggestions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Confidence {
    Safe,
    NeedsReview,
}

/// One structured suggestion: a message, a confidence, and the finalized
/// same-file text edits that realize it. Edits target the diagnostic's
/// own file; the stored form enforces that structurally by carrying
/// no file identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Suggestion {
    pub message: String,
    pub confidence: Confidence,
    pub edits: Vec<TextEdit>,
}

impl Ord for Suggestion {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (&self.message, self.confidence, &self.edits).cmp(&(
            &other.message,
            other.confidence,
            &other.edits,
        ))
    }
}

impl PartialOrd for Suggestion {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};

    use crate::{Confidence, Suggestion};

    fn suggestion(message: &str, confidence: Confidence) -> Suggestion {
        Suggestion {
            message: message.to_owned(),
            confidence,
            edits: vec![TextEdit {
                file: FileId::new(0),
                range: TextRange::new(TextSize::from(0), TextSize::from(4)),
                replacement: "save".to_owned(),
            }],
        }
    }

    #[test]
    fn safe_orders_below_needs_review() {
        assert!(Confidence::Safe < Confidence::NeedsReview);
    }

    #[test]
    fn suggestions_order_by_message_then_confidence() {
        let mut suggestions = [
            suggestion("beta", Confidence::Safe),
            suggestion("alpha", Confidence::NeedsReview),
            suggestion("alpha", Confidence::Safe),
        ];
        suggestions.sort();
        assert_eq!(
            suggestions
                .iter()
                .map(|suggestion| (suggestion.message.as_str(), suggestion.confidence))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", Confidence::Safe),
                ("alpha", Confidence::NeedsReview),
                ("beta", Confidence::Safe),
            ],
        );
    }
}
