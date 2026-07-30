//! The compiled stub index: every top-level symbol, deterministically
//! sorted, duplicates merged.

use crate::refinements::{RefinedClass, RefinedSignature, StubRefinements};
use crate::signature::{StubClassSurface, StubSignature};
use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol};

/// The compiled stub index, sorted by `(name, kind)`. `Eq`-comparable
/// so derived queries over it backdate (salsa early cutoff).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StubIndex {
    symbols: Vec<StubSymbol>,
    functions: Vec<(String, StubSignature)>,
    classes: Vec<(String, StubClassSurface)>,
    refinements: StubRefinements,
}

impl StubIndex {
    /// Builds the index: sorts all collections by name and deduplicates.
    ///
    /// **Symbols**: sorted by `(name, kind)` with duplicates merged into
    /// their availability union (phpstorm-stubs declares some symbols
    /// several times, with different availability guards).
    ///
    /// **Functions and classes**: sorted by name (stable); the first
    /// duplicate wins and later ones are silently dropped. This is a
    /// recorded simplification — phpstorm-stubs duplicate declarations
    /// carry the same shapes, so revisit if corpus spot checks disagree.
    pub fn new(
        mut symbols: Vec<StubSymbol>,
        mut functions: Vec<(String, StubSignature)>,
        mut classes: Vec<(String, StubClassSurface)>,
    ) -> Self {
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

        functions.sort_by(|left, right| left.0.cmp(&right.0));
        functions.dedup_by(|second, first| first.0 == second.0);
        classes.sort_by(|left, right| left.0.cmp(&right.0));
        classes.dedup_by(|second, first| first.0 == second.0);

        Self {
            symbols: merged,
            functions,
            classes,
            refinements: StubRefinements::empty(),
        }
    }

    /// Builds the index from symbols only; delegates to `new` with empty payloads.
    pub fn from_symbols(symbols: Vec<StubSymbol>) -> Self {
        Self::new(symbols, Vec::new(), Vec::new())
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

    pub fn functions(&self) -> &[(String, StubSignature)] {
        &self.functions
    }

    pub fn classes(&self) -> &[(String, StubClassSurface)] {
        &self.classes
    }

    /// Attaches the refinements overlay. A
    /// builder-style setter rather than a `new` parameter: `new`
    /// keeps its three-parameter shape so every existing caller
    /// compiles unchanged.
    pub fn set_refinements(&mut self, refinements: StubRefinements) {
        self.refinements = refinements;
    }

    pub fn refinements(&self) -> &StubRefinements {
        &self.refinements
    }

    pub fn function_refinement(&self, key: &str) -> Option<&RefinedSignature> {
        self.refinements
            .functions
            .binary_search_by(|(name, _)| name.as_str().cmp(key))
            .ok()
            .and_then(|position| self.refinements.functions.get(position))
            .map(|(_, signature)| signature)
    }

    pub fn class_refinement(&self, key: &str) -> Option<&RefinedClass> {
        self.refinements
            .classes
            .binary_search_by(|(name, _)| name.as_str().cmp(key))
            .ok()
            .and_then(|position| self.refinements.classes.get(position))
            .map(|(_, class)| class)
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
    use crate::refinements::{RefinedClass, RefinedSignature, StubRefinements};
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

    #[test]
    #[allow(clippy::indexing_slicing)]
    fn signature_payloads_sort_by_name_and_keep_the_first_duplicate() {
        use crate::signature::{StubClassSurface, StubSignature, VersionedTypeText};
        let first = StubSignature {
            return_type: VersionedTypeText::from_text(Some("int".to_owned())),
            ..StubSignature::default()
        };
        let second = StubSignature {
            return_type: VersionedTypeText::from_text(Some("string".to_owned())),
            ..StubSignature::default()
        };
        let index = StubIndex::new(
            vec![],
            vec![
                ("zebra".to_owned(), second.clone()),
                ("apple".to_owned(), first.clone()),
                ("apple".to_owned(), second),
            ],
            vec![("Exception".to_owned(), StubClassSurface::default())],
        );
        let names: Vec<&str> = index
            .functions()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, vec!["apple", "zebra"]);
        assert_eq!(index.functions()[0].1, first, "first duplicate wins");
        assert_eq!(index.classes().len(), 1);
    }

    #[test]
    fn a_freshly_built_index_carries_no_refinements() {
        // `new` initializes the overlay to empty even though it does
        // not take a refinements parameter: the boundary the brief's
        // `new` signature does not exercise on its own.
        let index = StubIndex::default();
        assert!(index.refinements().is_empty());
        assert_eq!(index.function_refinement("strlen"), None);
        assert_eq!(index.class_refinement("Exception"), None);
    }

    #[test]
    fn lookups_binary_search_among_several_sorted_entries() {
        let mut index = StubIndex::default();
        index.set_refinements(StubRefinements::new(
            vec![
                ("array_keys".to_owned(), RefinedSignature::default()),
                (
                    "strlen".to_owned(),
                    RefinedSignature {
                        return_type: Some("int".to_owned()),
                        ..RefinedSignature::default()
                    },
                ),
                ("zend_version".to_owned(), RefinedSignature::default()),
            ],
            vec![
                ("ArrayIterator".to_owned(), RefinedClass::default()),
                ("Exception".to_owned(), RefinedClass::default()),
                ("Traversable".to_owned(), RefinedClass::default()),
            ],
        ));
        // The middle entry, a boundary entry, and a key past every
        // entry (would land past the end in a naive binary search).
        assert_eq!(
            index
                .function_refinement("strlen")
                .and_then(|refinement| refinement.return_type.as_deref()),
            Some("int"),
        );
        assert!(index.function_refinement("array_keys").is_some());
        assert_eq!(index.function_refinement("zzz_not_present"), None);
        assert!(index.class_refinement("Exception").is_some());
        assert_eq!(index.class_refinement("Zzz\\NotPresent"), None);
    }
}
