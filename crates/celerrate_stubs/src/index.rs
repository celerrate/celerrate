//! The compiled stub index: every top-level symbol, deterministically
//! sorted, duplicates merged.

use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol};

/// The compiled stub index, sorted by `(name, kind)`. `Eq`-comparable
/// so derived queries over it backdate (salsa early cutoff).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StubIndex {
    symbols: Vec<StubSymbol>,
}

impl StubIndex {
    /// Builds the index: sorts by `(name, kind)` and merges duplicate
    /// declarations (phpstorm-stubs declares some symbols several
    /// times, with different availability guards) into their union.
    pub fn from_symbols(mut symbols: Vec<StubSymbol>) -> Self {
        symbols.sort_by(|left, right| left.name.cmp(&right.name).then(left.kind.cmp(&right.kind)));
        let mut merged: Vec<StubSymbol> = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            match merged.last_mut() {
                Some(last) if last.name == symbol.name && last.kind == symbol.kind => {
                    last.availability = merge_availability(last.availability, symbol.availability);
                }
                _ => merged.push(symbol),
            }
        }
        Self { symbols: merged }
    }

    pub fn symbols(&self) -> &[StubSymbol] {
        &self.symbols
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

/// The union of two availability windows: the widest wins. `None`
/// means "no constraint" and absorbs any bound; the merge is
/// deprecated only when every duplicate is deprecated.
fn merge_availability(left: StubAvailability, right: StubAvailability) -> StubAvailability {
    StubAvailability {
        introduced: match (left.introduced, right.introduced) {
            (Some(first), Some(second)) => Some(first.min(second)),
            _ => None,
        },
        removed: match (left.removed, right.removed) {
            (Some(first), Some(second)) => Some(first.max(second)),
            _ => None,
        },
        deprecated: match (left.deprecated, right.deprecated) {
            (Some(first), Some(second)) => Some(merge_deprecation(first, second)),
            _ => None,
        },
    }
}

fn merge_deprecation(left: StubDeprecation, right: StubDeprecation) -> StubDeprecation {
    StubDeprecation {
        since: match (left.since, right.since) {
            (Some(first), Some(second)) => Some(first.min(second)),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use celerrate_project::PhpVersion;

    use super::StubIndex;
    use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};

    fn symbol(name: &str, kind: StubSymbolKind, availability: StubAvailability) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind,
            availability,
        }
    }

    #[test]
    fn the_default_index_is_empty() {
        let index = StubIndex::default();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn symbols_sort_by_name_then_kind() {
        let index = StubIndex::from_symbols(vec![
            symbol("strlen", StubSymbolKind::Function, StubAvailability::ALWAYS),
            symbol(
                "Countable",
                StubSymbolKind::Interface,
                StubAvailability::ALWAYS,
            ),
            symbol("Countable", StubSymbolKind::Class, StubAvailability::ALWAYS),
        ]);
        let names: Vec<(&str, StubSymbolKind)> = index
            .symbols()
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Countable", StubSymbolKind::Class),
                ("Countable", StubSymbolKind::Interface),
                ("strlen", StubSymbolKind::Function),
            ],
        );
    }

    #[test]
    fn duplicate_declarations_merge_into_the_widest_window() {
        // phpstorm-stubs declares some symbols several times with
        // different availability guards: the union wins.
        let first = StubAvailability {
            introduced: Some(PhpVersion::new(8, 0)),
            removed: Some(PhpVersion::new(8, 2)),
            deprecated: Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 1)),
            }),
        };
        let second = StubAvailability {
            introduced: Some(PhpVersion::new(7, 4)),
            removed: Some(PhpVersion::new(8, 4)),
            deprecated: Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 0)),
            }),
        };
        let index = StubIndex::from_symbols(vec![
            symbol("foo", StubSymbolKind::Function, first),
            symbol("foo", StubSymbolKind::Function, second),
        ]);
        assert_eq!(index.len(), 1);
        let merged = index.symbols().first().map(|entry| entry.availability);
        assert_eq!(
            merged,
            Some(StubAvailability {
                introduced: Some(PhpVersion::new(7, 4)),
                removed: Some(PhpVersion::new(8, 4)),
                deprecated: Some(StubDeprecation {
                    since: Some(PhpVersion::new(8, 0)),
                }),
            }),
        );
    }

    #[test]
    fn no_constraint_absorbs_any_bound_when_merging() {
        let bounded = StubAvailability {
            introduced: Some(PhpVersion::new(8, 0)),
            removed: Some(PhpVersion::new(8, 2)),
            deprecated: Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 1)),
            }),
        };
        let index = StubIndex::from_symbols(vec![
            symbol("foo", StubSymbolKind::Function, bounded),
            symbol("foo", StubSymbolKind::Function, StubAvailability::ALWAYS),
        ]);
        assert_eq!(
            index.symbols().first().map(|entry| entry.availability),
            Some(StubAvailability::ALWAYS),
        );
    }

    #[test]
    fn same_name_different_kinds_stay_separate() {
        let index = StubIndex::from_symbols(vec![
            symbol(
                "Stringable",
                StubSymbolKind::Interface,
                StubAvailability::ALWAYS,
            ),
            symbol(
                "Stringable",
                StubSymbolKind::Class,
                StubAvailability::ALWAYS,
            ),
        ]);
        assert_eq!(index.len(), 2);
    }
}
