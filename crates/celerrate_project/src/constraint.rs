//! Composer version constraints, interpreted at minor precision.
//!
//! The engine reasons in `major.minor`, so every constraint is
//! truncated to that granularity with Composer's semantics preserved:
//! `>8.1.0` still admits `8.1.1` (minor 8.1 stays in), `<8.4.3` still
//! admits `8.4.0` (minor 8.4 stays in), `<8.4` excludes minor 8.4.
//! One documented deviation: a bare major (`"8"`) reads as the major
//! wildcard `8.*`, matching author intent in `require.php` rather than
//! Composer's exact-version normalization.

use crate::version::{PhpVersion, PhpVersionRange, SUPPORTED_VERSIONS};

/// Interprets a Composer constraint as the supported range it admits:
/// the lowest and highest supported versions satisfying it. `None`
/// when the constraint cannot be parsed or admits no supported
/// version; the caller decides the fallback and the notice.
pub fn version_range_for_constraint(constraint: &str) -> Option<PhpVersionRange> {
    let alternatives = parse_alternatives(constraint)?;
    let satisfied: Vec<PhpVersion> = SUPPORTED_VERSIONS
        .iter()
        .copied()
        .filter(|version| {
            alternatives.iter().any(|conjunction| {
                conjunction
                    .iter()
                    .all(|interval| interval.contains(*version))
            })
        })
        .collect();
    Some(PhpVersionRange::new(
        *satisfied.first()?,
        *satisfied.last()?,
    ))
}

/// Parses a plain version literal (`8.1`, `8.1.2`, `v8.3-dev`) at
/// minor precision, for `config.platform.php`. Bare majors, wildcards,
/// and operators are rejected: a platform names one concrete runtime.
pub fn php_version_from_text(text: &str) -> Option<PhpVersion> {
    let literal = parse_version_literal(text.trim())?;
    let minor = literal.minor?;
    Some(PhpVersion::new(literal.major, minor))
}

/// One inclusive-lower, exclusive-upper interval at minor precision.
/// `None` bounds are unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MinorInterval {
    lower_inclusive: Option<PhpVersion>,
    upper_exclusive: Option<PhpVersion>,
}

impl MinorInterval {
    const UNBOUNDED: Self = Self {
        lower_inclusive: None,
        upper_exclusive: None,
    };

    fn lower(version: PhpVersion) -> Self {
        Self {
            lower_inclusive: Some(version),
            upper_exclusive: None,
        }
    }

    fn upper(version: PhpVersion) -> Self {
        Self {
            lower_inclusive: None,
            upper_exclusive: Some(version),
        }
    }

    fn between(lower: PhpVersion, upper: PhpVersion) -> Self {
        Self {
            lower_inclusive: Some(lower),
            upper_exclusive: Some(upper),
        }
    }

    fn contains(self, version: PhpVersion) -> bool {
        self.lower_inclusive.is_none_or(|lower| version >= lower)
            && self.upper_exclusive.is_none_or(|upper| version < upper)
    }
}

/// A version literal: `8`, `8.1`, `8.1.2`, `v8.1`, `8.1.*`,
/// `8.1.2-beta1`. Pre-release and build suffixes are dropped;
/// components beyond the patch (Composer allows four) are ignored.
struct VersionLiteral {
    major: u8,
    /// `None`: bare major or a major-level wildcard.
    minor: Option<u8>,
    /// `None`: absent or a wildcard.
    patch: Option<u32>,
}

impl VersionLiteral {
    fn minor_version(&self) -> PhpVersion {
        PhpVersion::new(self.major, self.minor.unwrap_or(0))
    }
}

fn parse_version_literal(text: &str) -> Option<VersionLiteral> {
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    let text = text.split(['-', '+']).next()?;
    let mut parts = text.split('.');
    let major: u8 = parts.next()?.parse().ok()?;
    let minor = match parts.next() {
        None | Some("*" | "x" | "X") => None,
        Some(part) => Some(part.parse::<u8>().ok()?),
    };
    let patch = match parts.next() {
        None | Some("*" | "x" | "X") => None,
        Some(part) => Some(part.parse::<u32>().ok()?),
    };
    Some(VersionLiteral {
        major,
        minor,
        patch,
    })
}

fn next_minor(version: PhpVersion) -> Option<PhpVersion> {
    version
        .minor
        .checked_add(1)
        .map(|minor| PhpVersion::new(version.major, minor))
}

fn next_major(version: PhpVersion) -> Option<PhpVersion> {
    version
        .major
        .checked_add(1)
        .map(|major| PhpVersion::new(major, 0))
}

/// The alternatives (`||`, and legacy single `|`) of conjunctions.
fn parse_alternatives(constraint: &str) -> Option<Vec<Vec<MinorInterval>>> {
    let groups: Vec<&str> = constraint
        .split('|')
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .collect();
    if groups.is_empty() {
        return None;
    }
    groups.into_iter().map(parse_conjunction).collect()
}

/// One AND group: simples separated by whitespace or commas, with
/// spaced hyphen ranges (`8.1 - 8.3`) folded into single intervals.
fn parse_conjunction(text: &str) -> Option<Vec<MinorInterval>> {
    let replaced = text.replace(',', " ");
    let tokens: Vec<&str> = replaced.split_whitespace().collect();
    let mut intervals = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens.get(index)?;
        if tokens.get(index + 1).copied() == Some("-") {
            intervals.push(hyphen_range(token, tokens.get(index + 2)?)?);
            index += 3;
        } else {
            intervals.push(parse_simple(token)?);
            index += 1;
        }
    }
    if intervals.is_empty() {
        None
    } else {
        Some(intervals)
    }
}

fn hyphen_range(lower: &str, upper: &str) -> Option<MinorInterval> {
    let lower_literal = parse_version_literal(strip_stability(lower)?)?;
    let upper_literal = parse_version_literal(strip_stability(upper)?)?;
    let upper_version = upper_literal.minor_version();
    // A partial upper bound is completed by a wildcard, per Composer:
    // `8.1 - 8.3` is `>=8.1 <8.4`, `8.2 - 8` is `>=8.2 <9.0`.
    let upper_exclusive = if upper_literal.minor.is_some() {
        next_minor(upper_version)?
    } else {
        next_major(upper_version)?
    };
    Some(MinorInterval::between(
        lower_literal.minor_version(),
        upper_exclusive,
    ))
}

fn strip_stability(token: &str) -> Option<&str> {
    token.split('@').next()
}

fn parse_simple(token: &str) -> Option<MinorInterval> {
    let token = strip_stability(token)?;
    if token.is_empty() {
        return None;
    }
    if matches!(token, "*" | "x" | "X") {
        return Some(MinorInterval::UNBOUNDED);
    }
    if let Some(rest) = token.strip_prefix(">=") {
        return Some(MinorInterval::lower(
            parse_version_literal(rest)?.minor_version(),
        ));
    }
    if let Some(rest) = token.strip_prefix("<=") {
        let version = parse_version_literal(rest)?.minor_version();
        return Some(MinorInterval::upper(next_minor(version)?));
    }
    if let Some(rest) = token.strip_prefix('>') {
        // Composer's `>8.1.0` still admits `8.1.1`, so at minor
        // precision `>` behaves as `>=`.
        return Some(MinorInterval::lower(
            parse_version_literal(rest)?.minor_version(),
        ));
    }
    if let Some(rest) = token.strip_prefix('<') {
        let literal = parse_version_literal(rest)?;
        let version = literal.minor_version();
        let upper = if literal.patch.unwrap_or(0) > 0 {
            // `<8.4.3` still admits `8.4.0`.
            next_minor(version)?
        } else {
            version
        };
        return Some(MinorInterval::upper(upper));
    }
    if let Some(rest) = token.strip_prefix('^') {
        let version = parse_version_literal(rest)?.minor_version();
        return Some(MinorInterval::between(version, next_major(version)?));
    }
    if let Some(rest) = token.strip_prefix('~') {
        let literal = parse_version_literal(rest)?;
        let version = literal.minor_version();
        let upper = if literal.patch.is_some() {
            next_minor(version)?
        } else {
            next_major(version)?
        };
        return Some(MinorInterval::between(version, upper));
    }
    // A plain literal: `8` and `8.*` cover the major; `8.1` and
    // `8.1.2` pin the minor.
    let literal = parse_version_literal(token)?;
    let version = literal.minor_version();
    let upper = if literal.minor.is_some() {
        next_minor(version)?
    } else {
        next_major(version)?
    };
    Some(MinorInterval::between(version, upper))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{php_version_from_text, version_range_for_constraint};
    use crate::version::{PhpVersion, PhpVersionRange};

    fn range(
        minimum_major: u8,
        minimum_minor: u8,
        maximum_major: u8,
        maximum_minor: u8,
    ) -> PhpVersionRange {
        PhpVersionRange::new(
            PhpVersion::new(minimum_major, minimum_minor),
            PhpVersion::new(maximum_major, maximum_minor),
        )
    }

    #[test]
    fn caret_spans_to_the_end_of_the_major() {
        assert_eq!(
            version_range_for_constraint("^8.1"),
            Some(range(8, 1, 8, 5))
        );
        assert_eq!(
            version_range_for_constraint("^8.2.3"),
            Some(range(8, 2, 8, 5)),
        );
    }

    #[test]
    fn exact_versions_and_minor_wildcards_pin_the_minor() {
        assert_eq!(version_range_for_constraint("8.3"), Some(range(8, 3, 8, 3)));
        assert_eq!(
            version_range_for_constraint("8.2.7"),
            Some(range(8, 2, 8, 2)),
        );
        assert_eq!(
            version_range_for_constraint("8.2.*"),
            Some(range(8, 2, 8, 2)),
        );
    }

    #[test]
    fn a_bare_major_and_its_wildcard_cover_the_major() {
        assert_eq!(version_range_for_constraint("8"), Some(range(8, 1, 8, 5)));
        assert_eq!(version_range_for_constraint("8.*"), Some(range(8, 1, 8, 5)));
        assert_eq!(version_range_for_constraint("*"), Some(range(8, 1, 8, 5)));
    }

    #[test]
    fn comparisons_combine_with_spaces_or_commas_as_and() {
        assert_eq!(
            version_range_for_constraint(">=8.2 <8.5"),
            Some(range(8, 2, 8, 4)),
        );
        assert_eq!(
            version_range_for_constraint(">=8.2,<=8.4"),
            Some(range(8, 2, 8, 4)),
        );
    }

    #[test]
    fn strict_bounds_follow_the_minor_precision_truncation_rules() {
        // `>8.1.0` admits `8.1.1`, so minor 8.1 stays in.
        assert_eq!(
            version_range_for_constraint(">8.1"),
            Some(range(8, 1, 8, 5))
        );
        // `<8.4.0` excludes all of minor 8.4 ...
        assert_eq!(
            version_range_for_constraint("<8.4"),
            Some(range(8, 1, 8, 3))
        );
        // ... but `<8.4.3` still admits `8.4.0`.
        assert_eq!(
            version_range_for_constraint("<8.4.3"),
            Some(range(8, 1, 8, 4)),
        );
    }

    #[test]
    fn tilde_bumps_the_component_below_the_last_named_one() {
        assert_eq!(
            version_range_for_constraint("~8.1"),
            Some(range(8, 1, 8, 5))
        );
        assert_eq!(
            version_range_for_constraint("~8.1.0"),
            Some(range(8, 1, 8, 1)),
        );
    }

    #[test]
    fn hyphen_ranges_complete_a_partial_upper_bound_with_a_wildcard() {
        assert_eq!(
            version_range_for_constraint("8.1 - 8.3"),
            Some(range(8, 1, 8, 3)),
        );
        assert_eq!(
            version_range_for_constraint("8.2 - 8"),
            Some(range(8, 2, 8, 5)),
        );
    }

    #[test]
    fn alternatives_take_the_union() {
        assert_eq!(
            version_range_for_constraint("^7.4 || ^8.0"),
            Some(range(8, 1, 8, 5)),
        );
    }

    #[test]
    fn stability_flags_and_prefixes_are_ignored() {
        assert_eq!(
            version_range_for_constraint("^8.1@dev"),
            Some(range(8, 1, 8, 5)),
        );
        assert_eq!(
            version_range_for_constraint("v8.2"),
            Some(range(8, 2, 8, 2)),
        );
        assert_eq!(
            version_range_for_constraint("8.1.0-beta1"),
            Some(range(8, 1, 8, 1)),
        );
    }

    #[test]
    fn unparseable_and_unsatisfiable_constraints_are_rejected() {
        assert_eq!(version_range_for_constraint("banana"), None);
        assert_eq!(version_range_for_constraint(""), None);
        assert_eq!(version_range_for_constraint("!=8.1"), None);
        // Parseable, but admits no supported version.
        assert_eq!(version_range_for_constraint("7.4.*"), None);
    }

    #[test]
    fn a_malformed_alternative_poisons_the_whole_constraint() {
        // Fail-closed by decision: one unparseable alternative makes
        // the whole constraint unparseable, and the caller reports it.
        assert_eq!(version_range_for_constraint("banana || ^8.1"), None);
    }

    #[test]
    fn a_platform_version_is_a_concrete_literal() {
        assert_eq!(php_version_from_text("8.1.2"), Some(PhpVersion::new(8, 1)));
        assert_eq!(
            php_version_from_text("v8.3-dev"),
            Some(PhpVersion::new(8, 3))
        );
        assert_eq!(php_version_from_text("8"), None);
        assert_eq!(php_version_from_text("8.*"), None);
        assert_eq!(php_version_from_text("^8.1"), None);
    }
}
