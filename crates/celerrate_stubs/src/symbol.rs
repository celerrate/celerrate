//! The stub symbol model: what one top-level declaration of the
//! compiled phpstorm-stubs snapshot looks like at runtime.

use celerrate_project::{PhpVersion, PhpVersionRange};

/// The kind of a top-level stub symbol. The discriminants are the blob
/// encoding: fixed forever once a blob format version has shipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StubSymbolKind {
    Class = 0,
    Interface = 1,
    Trait = 2,
    Enum = 3,
    Function = 4,
    Constant = 5,
}

impl StubSymbolKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Class),
            1 => Some(Self::Interface),
            2 => Some(Self::Trait),
            3 => Some(Self::Enum),
            4 => Some(Self::Function),
            5 => Some(Self::Constant),
            _ => None,
        }
    }
}

/// A deprecation mark. `since` is the version that deprecated the
/// symbol; `None` when the stubs mark a deprecation without a version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StubDeprecation {
    pub since: Option<PhpVersion>,
}

/// Per-version availability of one symbol. `None` means "no
/// constraint". `removed` is the first version in which the symbol no
/// longer exists (the `@removed` convention of phpstorm-stubs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StubAvailability {
    pub introduced: Option<PhpVersion>,
    pub removed: Option<PhpVersion>,
    pub deprecated: Option<StubDeprecation>,
}

impl StubAvailability {
    pub const ALWAYS: Self = Self {
        introduced: None,
        removed: None,
        deprecated: None,
    };

    /// Whether the symbol exists anywhere in `range`: introduced no
    /// later than the maximum and not yet removed at the minimum.
    /// Deprecation never affects existence.
    pub fn exists_in(&self, range: PhpVersionRange) -> bool {
        self.introduced
            .is_none_or(|version| version <= range.maximum)
            && self.removed.is_none_or(|version| version > range.minimum)
    }
}

/// One top-level symbol compiled from the stubs: the fully qualified
/// name (original spelling, no leading backslash), its kind, and its
/// availability window.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StubSymbol {
    pub name: String,
    pub kind: StubSymbolKind,
    pub availability: StubAvailability,
}

#[cfg(test)]
mod tests {
    use celerrate_project::{PhpVersion, PhpVersionRange};

    use super::{StubAvailability, StubDeprecation, StubSymbolKind};

    #[test]
    fn kinds_round_trip_through_their_blob_discriminants() {
        let kinds = [
            StubSymbolKind::Class,
            StubSymbolKind::Interface,
            StubSymbolKind::Trait,
            StubSymbolKind::Enum,
            StubSymbolKind::Function,
            StubSymbolKind::Constant,
        ];
        for (expected_discriminant, kind) in kinds.into_iter().enumerate() {
            assert_eq!(usize::from(kind.as_u8()), expected_discriminant);
            assert_eq!(StubSymbolKind::from_u8(kind.as_u8()), Some(kind));
        }
        assert_eq!(StubSymbolKind::from_u8(6), None);
    }

    #[test]
    fn an_unconstrained_symbol_exists_in_every_range() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        assert!(StubAvailability::ALWAYS.exists_in(range));
    }

    #[test]
    fn a_symbol_introduced_inside_the_range_exists() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let availability = StubAvailability {
            introduced: Some(PhpVersion::new(8, 3)),
            ..StubAvailability::ALWAYS
        };
        assert!(availability.exists_in(range));
    }

    #[test]
    fn a_symbol_introduced_after_the_maximum_does_not_exist() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 2));
        let availability = StubAvailability {
            introduced: Some(PhpVersion::new(8, 3)),
            ..StubAvailability::ALWAYS
        };
        assert!(!availability.exists_in(range));
    }

    #[test]
    fn a_symbol_removed_at_or_before_the_minimum_does_not_exist() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let removed_before = StubAvailability {
            removed: Some(PhpVersion::new(8, 0)),
            ..StubAvailability::ALWAYS
        };
        let removed_at_minimum = StubAvailability {
            removed: Some(PhpVersion::new(8, 1)),
            ..StubAvailability::ALWAYS
        };
        assert!(!removed_before.exists_in(range));
        assert!(!removed_at_minimum.exists_in(range));
    }

    #[test]
    fn a_symbol_removed_inside_the_range_still_exists_at_the_minimum() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let availability = StubAvailability {
            removed: Some(PhpVersion::new(8, 3)),
            ..StubAvailability::ALWAYS
        };
        assert!(availability.exists_in(range));
    }

    #[test]
    fn deprecation_never_affects_existence() {
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let availability = StubAvailability {
            deprecated: Some(StubDeprecation {
                since: Some(PhpVersion::new(8, 1)),
            }),
            ..StubAvailability::ALWAYS
        };
        assert!(availability.exists_in(range));
    }
}
