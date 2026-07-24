//! The canonical identifier registry: every identifier Celerrate has
//! allocated, the family it belongs to, and the crate that produces it.
//!
//! This is the list a human reads, and the list `celerrate_cli` checks
//! the producers against. It exists because a strict dependency DAG
//! leaves no layer below the composition root able to see two producers
//! at once: `celerrate_project` and `celerrate_semantics` both allocated
//! `CEL0018`, both had a passing stability test, and neither could
//! notice. An identifier is permanent once published: a new diagnostic
//! takes the next free number and never reuses a retired one.

use crate::explain::ExplainPage;
use crate::identifier::DiagnosticId;
use crate::pages;

/// One allocated identifier: what it means, and who produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredDiagnostic {
    pub id: DiagnosticId,
    pub family: &'static str,
    pub owner: &'static str,
    /// The long-form explanation served by `celerrate explain`.
    pub explain: &'static ExplainPage,
}

/// The constructor for every registry entry: an identifier is never
/// allocated without the page that explains it.
const fn registered(
    id: &'static str,
    family: &'static str,
    owner: &'static str,
    explain: &'static ExplainPage,
) -> RegisteredDiagnostic {
    RegisteredDiagnostic {
        id: DiagnosticId::new(id),
        family,
        owner,
        explain,
    }
}

/// Every identifier, in identifier order.
pub const REGISTRY: &[RegisteredDiagnostic] = &[
    registered(
        "CEL0001",
        "source too large",
        "celerrate_db",
        &pages::source::CEL0001,
    ),
    registered(
        "CEL0002",
        "unexpected character",
        "celerrate_syntax",
        &pages::syntax::CEL0002,
    ),
    registered(
        "CEL0003",
        "unterminated block comment",
        "celerrate_syntax",
        &pages::syntax::CEL0003,
    ),
    registered(
        "CEL0004",
        "unterminated string",
        "celerrate_syntax",
        &pages::syntax::CEL0004,
    ),
    registered(
        "CEL0005",
        "unterminated heredoc",
        "celerrate_syntax",
        &pages::syntax::CEL0005,
    ),
    registered(
        "CEL0006",
        "unterminated interpolation",
        "celerrate_syntax",
        &pages::syntax::CEL0006,
    ),
    registered(
        "CEL0007",
        "expected an expression",
        "celerrate_syntax",
        &pages::syntax::CEL0007,
    ),
    registered(
        "CEL0008",
        "expected a semicolon",
        "celerrate_syntax",
        &pages::syntax::CEL0008,
    ),
    registered(
        "CEL0009",
        "expected a specific token",
        "celerrate_syntax",
        &pages::syntax::CEL0009,
    ),
    registered(
        "CEL0010",
        "unexpected token",
        "celerrate_syntax",
        &pages::syntax::CEL0010,
    ),
    registered(
        "CEL0011",
        "nesting too deep",
        "celerrate_syntax",
        &pages::syntax::CEL0011,
    ),
    registered(
        "CEL0012",
        "non-associative operator chained",
        "celerrate_syntax",
        &pages::syntax::CEL0012,
    ),
    registered(
        "CEL0013",
        "no progress",
        "celerrate_syntax",
        &pages::syntax::CEL0013,
    ),
    registered(
        "CEL0014",
        "expected a member name",
        "celerrate_syntax",
        &pages::syntax::CEL0014,
    ),
    registered(
        "CEL0015",
        "expected a statement",
        "celerrate_syntax",
        &pages::syntax::CEL0015,
    ),
    registered(
        "CEL0016",
        "expected a type",
        "celerrate_syntax",
        &pages::syntax::CEL0016,
    ),
    registered(
        "CEL0017",
        "expected a declaration",
        "celerrate_syntax",
        &pages::syntax::CEL0017,
    ),
    registered(
        "CEL0018",
        "unknown class",
        "celerrate_rules",
        &pages::semantic::CEL0018,
    ),
    registered(
        "CEL0019",
        "unknown function",
        "celerrate_rules",
        &pages::semantic::CEL0019,
    ),
    registered(
        "CEL0020",
        "unknown constant",
        "celerrate_rules",
        &pages::semantic::CEL0020,
    ),
    registered(
        "CEL0021",
        "symbol not available",
        "celerrate_rules",
        &pages::semantic::CEL0021,
    ),
    registered(
        "CEL0022",
        "symbol removed",
        "celerrate_rules",
        &pages::semantic::CEL0022,
    ),
    registered(
        "CEL0023",
        "symbol deprecated",
        "celerrate_rules",
        &pages::semantic::CEL0023,
    ),
    registered(
        "CEL0024",
        "syntax construct not available",
        "celerrate_rules",
        &pages::semantic::CEL0024,
    ),
    registered(
        "CEL0025",
        "missing Composer manifest",
        "celerrate_project",
        &pages::project::CEL0025,
    ),
    registered(
        "CEL0026",
        "invalid Composer manifest",
        "celerrate_project",
        &pages::project::CEL0026,
    ),
    registered(
        "CEL0027",
        "PHP version fallback",
        "celerrate_project",
        &pages::project::CEL0027,
    ),
    registered(
        "CEL0028",
        "invalid PHP version constraint",
        "celerrate_project",
        &pages::project::CEL0028,
    ),
    registered(
        "CEL0029",
        "invalid installed packages",
        "celerrate_project",
        &pages::project::CEL0029,
    ),
    registered(
        "CEL0030",
        "unknown method",
        "celerrate_rules",
        &pages::typed::CEL0030,
    ),
    registered(
        "CEL0031",
        "unknown property",
        "celerrate_rules",
        &pages::typed::CEL0031,
    ),
    registered(
        "CEL0032",
        "unknown class constant",
        "celerrate_rules",
        &pages::typed::CEL0032,
    ),
    registered(
        "CEL0033",
        "unknown enum case",
        "celerrate_rules",
        &pages::typed::CEL0033,
    ),
    registered(
        "CEL0034",
        "possibly null dereference",
        "celerrate_rules",
        &pages::typed::CEL0034,
    ),
    registered(
        "CEL0035",
        "argument type mismatch",
        "celerrate_rules",
        &pages::typed::CEL0035,
    ),
    registered(
        "CEL0036",
        "too few arguments",
        "celerrate_rules",
        &pages::typed::CEL0036,
    ),
    registered(
        "CEL0037",
        "too many arguments",
        "celerrate_rules",
        &pages::typed::CEL0037,
    ),
    registered(
        "CEL0038",
        "unknown named argument",
        "celerrate_rules",
        &pages::typed::CEL0038,
    ),
    registered(
        "CEL0039",
        "unreadable Composer manifest",
        "celerrate_project",
        &pages::project::CEL0039,
    ),
    registered(
        "CEL0040",
        "unreadable installed packages",
        "celerrate_project",
        &pages::project::CEL0040,
    ),
    registered(
        "CEL0041",
        "unknown suppression identifier",
        "celerrate_rules",
        &pages::reporting::CEL0041,
    ),
    registered(
        "CEL0042",
        "unused suppression",
        "celerrate_rules",
        &pages::reporting::CEL0042,
    ),
];

/// The registered identifier whose text is `text`, re-interned to its
/// `'static` form. `None` for anything the registry does not know: a
/// deserialized identifier that fails this lookup comes from another
/// binary's era and its carrier is discarded, never guessed at.
pub fn find_identifier(text: &str) -> Option<DiagnosticId> {
    REGISTRY
        .iter()
        .find(|entry| entry.id.as_str() == text)
        .map(|entry| entry.id)
}

/// The explain page registered for `id`. `None` now only means an
/// unknown identifier.
pub fn find_page(id: DiagnosticId) -> Option<&'static ExplainPage> {
    REGISTRY
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.explain)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::{REGISTRY, find_identifier};

    #[test]
    fn the_registry_is_sorted_unique_and_gapless() {
        let mut previous = 0u32;
        for entry in REGISTRY {
            let text = entry.id.as_str();
            assert!(text.starts_with("CEL"), "malformed identifier {text}");
            let number: u32 = text
                .strip_prefix("CEL")
                .and_then(|digits| digits.parse().ok())
                .unwrap();
            assert_eq!(
                number,
                previous + 1,
                "identifiers are allocated in one gapless run: {text} follows CEL{previous:04}",
            );
            previous = number;
        }
        assert_eq!(previous, 42, "forty-two identifiers allocated so far");
    }

    #[test]
    fn every_entry_names_a_family_and_an_owner() {
        for entry in REGISTRY {
            assert!(
                !entry.family.is_empty(),
                "{} has no family",
                entry.id.as_str()
            );
            assert!(
                entry.owner.starts_with("celerrate_"),
                "{} has no owning crate",
                entry.id.as_str(),
            );
        }
    }

    #[test]
    fn a_registered_identifier_is_found_and_an_unknown_one_is_not() {
        let found = find_identifier("CEL0018").unwrap();
        assert_eq!(found.as_str(), "CEL0018");
        assert!(find_identifier("CEL9999").is_none());
        assert!(find_identifier("").is_none());
    }

    #[test]
    fn every_identifier_has_a_page_with_all_four_sections() {
        for entry in REGISTRY {
            let page = entry.explain;
            for (section, text) in [
                ("why", page.why),
                ("failing example", page.failing_example),
                ("fixed example", page.fixed_example),
                ("configuration", page.configuration),
            ] {
                assert!(
                    !text.trim().is_empty(),
                    "{} has an empty {section} section",
                    entry.id.as_str(),
                );
            }
        }
    }
}
