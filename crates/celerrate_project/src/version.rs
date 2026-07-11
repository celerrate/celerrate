use core::fmt;

/// A PHP version at the granularity the engine reasons about:
/// `major.minor`. Patch components are truncated on input; version
/// gating and availability metadata are minor-granular.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhpVersion {
    pub major: u8,
    pub minor: u8,
}

impl PhpVersion {
    pub const fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }

    /// Folds into the supported window: older than the oldest
    /// supported becomes the oldest, newer than the latest stable
    /// becomes the latest.
    pub fn clamped_to_supported(self) -> Self {
        self.clamp(OLDEST_SUPPORTED_VERSION, LATEST_STABLE_VERSION)
    }
}

impl fmt::Display for PhpVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

/// The versions this binary knows, oldest to newest. Bumped
/// deliberately when a new PHP minor becomes supported, like the
/// pinned corpus and stub snapshots.
pub const SUPPORTED_VERSIONS: [PhpVersion; 5] = [
    PhpVersion::new(8, 1),
    PhpVersion::new(8, 2),
    PhpVersion::new(8, 3),
    PhpVersion::new(8, 4),
    PhpVersion::new(8, 5),
];

/// The oldest version the engine supports (parent spec: PHP 8.1+).
pub const OLDEST_SUPPORTED_VERSION: PhpVersion = PhpVersion::new(8, 1);

/// The newest supported stable version: the zero-configuration
/// fallback when no version signal exists.
pub const LATEST_STABLE_VERSION: PhpVersion = PhpVersion::new(8, 5);

/// The supported PHP version range `[minimum, maximum]`, inclusive.
/// Availability checks run at the minimum, removal and deprecation
/// checks at the maximum (parent spec's range rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhpVersionRange {
    pub minimum: PhpVersion,
    pub maximum: PhpVersion,
}

impl PhpVersionRange {
    pub const fn new(minimum: PhpVersion, maximum: PhpVersion) -> Self {
        Self { minimum, maximum }
    }

    /// A range collapsed to a single version.
    pub const fn point(version: PhpVersion) -> Self {
        Self::new(version, version)
    }

    /// The zero-configuration fallback: the latest supported stable
    /// version, as a point.
    pub const fn fallback() -> Self {
        Self::point(LATEST_STABLE_VERSION)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{
        LATEST_STABLE_VERSION, OLDEST_SUPPORTED_VERSION, PhpVersion, PhpVersionRange,
        SUPPORTED_VERSIONS,
    };

    #[test]
    fn a_version_displays_as_major_dot_minor() {
        assert_eq!(PhpVersion::new(8, 1).to_string(), "8.1");
    }

    #[test]
    fn versions_order_by_major_then_minor() {
        assert!(PhpVersion::new(8, 1) < PhpVersion::new(8, 2));
        assert!(PhpVersion::new(7, 9) < PhpVersion::new(8, 0));
    }

    #[test]
    fn the_supported_window_is_php_8_1_through_8_5_ascending() {
        assert_eq!(SUPPORTED_VERSIONS.first(), Some(&OLDEST_SUPPORTED_VERSION));
        assert_eq!(SUPPORTED_VERSIONS.last(), Some(&LATEST_STABLE_VERSION));
        assert_eq!(OLDEST_SUPPORTED_VERSION, PhpVersion::new(8, 1));
        assert_eq!(LATEST_STABLE_VERSION, PhpVersion::new(8, 5));
        assert!(SUPPORTED_VERSIONS.is_sorted());
    }

    #[test]
    fn clamping_folds_into_the_supported_window() {
        assert_eq!(
            PhpVersion::new(7, 4).clamped_to_supported(),
            PhpVersion::new(8, 1),
        );
        assert_eq!(
            PhpVersion::new(9, 0).clamped_to_supported(),
            PhpVersion::new(8, 5),
        );
        assert_eq!(
            PhpVersion::new(8, 3).clamped_to_supported(),
            PhpVersion::new(8, 3),
        );
    }

    #[test]
    fn a_point_range_pins_both_ends_and_fallback_is_the_latest_stable() {
        let point = PhpVersionRange::point(PhpVersion::new(8, 2));
        assert_eq!(point.minimum, PhpVersion::new(8, 2));
        assert_eq!(point.maximum, PhpVersion::new(8, 2));
        assert_eq!(
            PhpVersionRange::fallback(),
            PhpVersionRange::point(LATEST_STABLE_VERSION),
        );
    }
}
