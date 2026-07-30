use celerrate_source::TextRange;

/// Where a secondary label points.
///
/// A label in the primary span's own file carries its concrete range. A
/// label in another file (or in a stub, which has no source at all) is
/// carried symbolically: the referenced declaration's display path,
/// resolved to a concrete location at render time, outside queries. The
/// symbolic form is deliberate and load-bearing: a concrete range of
/// another file embedded in a per-file artifact goes stale invisibly, and
/// resolving it inside a query would pierce the range-free invalidation
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LabelTarget {
    Local { range: TextRange },
    Symbolic { symbol: String },
}

impl Ord for LabelTarget {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        match (self, other) {
            (Self::Local { range: left }, Self::Local { range: right }) => {
                (left.start(), left.end()).cmp(&(right.start(), right.end()))
            }
            (Self::Local { .. }, Self::Symbolic { .. }) => core::cmp::Ordering::Less,
            (Self::Symbolic { .. }, Self::Local { .. }) => core::cmp::Ordering::Greater,
            (Self::Symbolic { symbol: left }, Self::Symbolic { symbol: right }) => left.cmp(right),
        }
    }
}

impl PartialOrd for LabelTarget {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// One secondary annotated span: "the parameter is declared `int` here".
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label {
    pub target: LabelTarget,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use celerrate_source::{TextRange, TextSize};

    use crate::{Label, LabelTarget};

    fn local(start: u32, end: u32, message: &str) -> Label {
        Label {
            target: LabelTarget::Local {
                range: TextRange::new(TextSize::from(start), TextSize::from(end)),
            },
            message: message.to_owned(),
        }
    }

    fn symbolic(symbol: &str, message: &str) -> Label {
        Label {
            target: LabelTarget::Symbolic {
                symbol: symbol.to_owned(),
            },
            message: message.to_owned(),
        }
    }

    #[test]
    fn local_labels_order_before_symbolic_ones() {
        let mut labels = [
            symbolic("App\\User::save", "declared here"),
            local(0, 4, "here"),
        ];
        labels.sort();
        assert!(matches!(
            labels.first().map(|label| &label.target),
            Some(LabelTarget::Local { .. })
        ));
    }

    #[test]
    fn labels_order_by_target_then_message() {
        let mut labels = [
            local(0, 4, "beta"),
            local(0, 4, "alpha"),
            local(0, 2, "zeta"),
        ];
        labels.sort();
        assert_eq!(
            labels
                .iter()
                .map(|label| label.message.as_str())
                .collect::<Vec<_>>(),
            vec!["zeta", "alpha", "beta"],
        );
    }
}
