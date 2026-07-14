//! The versioned stub signature model: type texts with per-version
//! overrides, parameters, signatures, members, and class surfaces.

use celerrate_project::PhpVersion;

use crate::symbol::StubAvailability;

/// One type text across PHP versions: a default plus ascending
/// `(from_version, text)` overrides — the compiled form of
/// phpstorm-stubs' `#[LanguageLevelTypeAware]`. `at(v)` answers the
/// text effective at `v`: the last override whose version is ≤ `v`,
/// else the default. `NONE` (no default, no overrides) means "no
/// declared type at any version".
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct VersionedTypeText {
    pub default: Option<String>,
    pub overrides: Vec<(PhpVersion, String)>,
}

impl VersionedTypeText {
    /// A plain, unversioned text (or nothing).
    pub fn from_text(text: Option<String>) -> Self {
        Self {
            default: text,
            overrides: Vec::new(),
        }
    }

    /// The text effective at `version`: the last override whose
    /// version is not later, else the default. Overrides are kept
    /// sorted ascending by the constructors (Task 9's extractor sorts;
    /// Task 8's decoder preserves order).
    pub fn at(&self, version: PhpVersion) -> Option<&str> {
        self.overrides
            .iter()
            .rev()
            .find(|(from, _)| *from <= version)
            .map(|(_, text)| text.as_str())
            .or(self.default.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StubParameter {
    pub name: String,
    pub type_text: VersionedTypeText,
    pub optional: bool,
    pub by_reference: bool,
    pub variadic: bool,
    /// The parameter's own window (`#[PhpStormStubsElementAvailable]`):
    /// a parameter added in 8.2 exists only from 8.2 on.
    pub availability: StubAvailability,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StubSignature {
    pub parameters: Vec<StubParameter>,
    pub return_type: VersionedTypeText,
    pub by_reference: bool,
}

/// Blob discriminants: fixed forever, like `StubSymbolKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StubMemberKind {
    Method = 0,
    Property = 1,
    ClassConstant = 2,
    EnumCase = 3,
}

impl StubMemberKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Method),
            1 => Some(Self::Property),
            2 => Some(Self::ClassConstant),
            3 => Some(Self::EnumCase),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StubVisibility {
    Public = 0,
    Protected = 1,
    Private = 2,
}

impl StubVisibility {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Public),
            1 => Some(Self::Protected),
            2 => Some(Self::Private),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StubMember {
    pub kind: StubMemberKind,
    /// Original spelling; property names without the `$`.
    pub name: String,
    pub visibility: StubVisibility,
    pub is_static: bool,
    pub availability: StubAvailability,
    /// Methods only. The blob encoding (`blob.rs`) has no `None`
    /// variant on the wire: a `None` signature is written as an empty
    /// `StubSignature`, and every method decodes back to
    /// `Some(signature)` — `None` only ever appears here between
    /// construction and encoding, never after a round trip.
    pub signature: Option<StubSignature>,
    /// Properties and class constants: the declared/versioned type.
    pub type_text: VersionedTypeText,
    /// Class constants and enum cases: the literal value text, when
    /// the value is a simple literal (`'active'`, `- 1`), else `None`.
    pub value_text: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StubClassSurface {
    /// Fully qualified parent names (no leading backslash), extends
    /// first then implements, declared order — the walk order of the
    /// stub side of linearization.
    pub parents: Vec<String>,
    pub members: Vec<StubMember>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_project::PhpVersion;

    use super::{StubMemberKind, StubVisibility, VersionedTypeText};

    #[test]
    fn a_versioned_text_answers_the_effective_text_per_version() {
        let text = VersionedTypeText {
            default: Some("int".to_owned()),
            overrides: vec![
                (PhpVersion::new(8, 0), "int|false".to_owned()),
                (PhpVersion::new(8, 3), "int|float|false".to_owned()),
            ],
        };
        assert_eq!(text.at(PhpVersion::new(7, 4)), Some("int"));
        assert_eq!(text.at(PhpVersion::new(8, 0)), Some("int|false"));
        assert_eq!(text.at(PhpVersion::new(8, 2)), Some("int|false"));
        assert_eq!(text.at(PhpVersion::new(8, 3)), Some("int|float|false"));
        assert_eq!(text.at(PhpVersion::new(8, 5)), Some("int|float|false"));
    }

    #[test]
    fn the_empty_versioned_text_is_none_everywhere() {
        assert_eq!(VersionedTypeText::default().at(PhpVersion::new(8, 1)), None);
        assert_eq!(
            VersionedTypeText::from_text(Some("string".to_owned())).at(PhpVersion::new(8, 1)),
            Some("string"),
        );
        assert_eq!(
            VersionedTypeText::from_text(None).at(PhpVersion::new(8, 1)),
            None
        );
    }

    #[test]
    fn member_kinds_and_visibilities_round_trip_their_discriminants() {
        for kind in [
            StubMemberKind::Method,
            StubMemberKind::Property,
            StubMemberKind::ClassConstant,
            StubMemberKind::EnumCase,
        ] {
            assert_eq!(StubMemberKind::from_u8(kind.as_u8()), Some(kind));
        }
        assert_eq!(StubMemberKind::from_u8(4), None);
        for visibility in [
            StubVisibility::Public,
            StubVisibility::Protected,
            StubVisibility::Private,
        ] {
            assert_eq!(
                StubVisibility::from_u8(visibility.as_u8()),
                Some(visibility)
            );
        }
        assert_eq!(StubVisibility::from_u8(3), None);
    }
}
