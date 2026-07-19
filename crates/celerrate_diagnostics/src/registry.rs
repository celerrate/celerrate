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

use crate::identifier::DiagnosticId;

/// One allocated identifier: what it means, and who produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredDiagnostic {
    pub id: DiagnosticId,
    pub family: &'static str,
    pub owner: &'static str,
}

const fn registered(
    id: &'static str,
    family: &'static str,
    owner: &'static str,
) -> RegisteredDiagnostic {
    RegisteredDiagnostic {
        id: DiagnosticId::new(id),
        family,
        owner,
    }
}

/// Every identifier, in identifier order.
pub const REGISTRY: &[RegisteredDiagnostic] = &[
    registered("CEL0001", "source too large", "celerrate_db"),
    registered("CEL0002", "unexpected character", "celerrate_syntax"),
    registered("CEL0003", "unterminated block comment", "celerrate_syntax"),
    registered("CEL0004", "unterminated string", "celerrate_syntax"),
    registered("CEL0005", "unterminated heredoc", "celerrate_syntax"),
    registered("CEL0006", "unterminated interpolation", "celerrate_syntax"),
    registered("CEL0007", "expected an expression", "celerrate_syntax"),
    registered("CEL0008", "expected a semicolon", "celerrate_syntax"),
    registered("CEL0009", "expected a specific token", "celerrate_syntax"),
    registered("CEL0010", "unexpected token", "celerrate_syntax"),
    registered("CEL0011", "nesting too deep", "celerrate_syntax"),
    registered(
        "CEL0012",
        "non-associative operator chained",
        "celerrate_syntax",
    ),
    registered("CEL0013", "no progress", "celerrate_syntax"),
    registered("CEL0014", "expected a member name", "celerrate_syntax"),
    registered("CEL0015", "expected a statement", "celerrate_syntax"),
    registered("CEL0016", "expected a type", "celerrate_syntax"),
    registered("CEL0017", "expected a declaration", "celerrate_syntax"),
    registered("CEL0018", "unknown class", "celerrate_semantics"),
    registered("CEL0019", "unknown function", "celerrate_semantics"),
    registered("CEL0020", "unknown constant", "celerrate_semantics"),
    registered("CEL0021", "symbol not available", "celerrate_semantics"),
    registered("CEL0022", "symbol removed", "celerrate_semantics"),
    registered("CEL0023", "symbol deprecated", "celerrate_semantics"),
    registered(
        "CEL0024",
        "syntax construct not available",
        "celerrate_semantics",
    ),
    registered("CEL0025", "missing Composer manifest", "celerrate_project"),
    registered("CEL0026", "invalid Composer manifest", "celerrate_project"),
    registered("CEL0027", "PHP version fallback", "celerrate_project"),
    registered(
        "CEL0028",
        "invalid PHP version constraint",
        "celerrate_project",
    ),
    registered("CEL0029", "invalid installed packages", "celerrate_project"),
    registered("CEL0030", "unknown method", "celerrate_types"),
    registered("CEL0031", "unknown property", "celerrate_types"),
    registered("CEL0032", "unknown class constant", "celerrate_types"),
    registered("CEL0033", "unknown enum case", "celerrate_types"),
    registered("CEL0034", "possibly null dereference", "celerrate_types"),
    registered("CEL0035", "argument type mismatch", "celerrate_types"),
    registered("CEL0036", "too few arguments", "celerrate_types"),
    registered("CEL0037", "too many arguments", "celerrate_types"),
    registered("CEL0038", "unknown named argument", "celerrate_types"),
    registered(
        "CEL0039",
        "unreadable Composer manifest",
        "celerrate_project",
    ),
    registered(
        "CEL0040",
        "unreadable installed packages",
        "celerrate_project",
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
        assert_eq!(previous, 40, "forty identifiers allocated so far");
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
}
