//! The correspondence-table triage gate (design section 8's closure
//! gate): both dialects' published catalogues fully triaged - table
//! and catalogue are the same set, both directions - and every mapped
//! code re-interns, so silent widening through table incompleteness is
//! bounded by review, never by accident. Lives at the composition root
//! because the bridge may not depend on `celerrate_diagnostics` (the
//! dependency-shape gate) and so cannot check its own code strings.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use celerrate_phpdoc_bridge::{Dialect, ForeignMapping, correspondence_entries};

fn catalogue(file: &str) -> BTreeSet<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../celerrate_phpdoc_bridge/catalogues")
        .join(file);
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is unreadable: {error}", path.display()));
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn table_keys(dialect: Dialect) -> BTreeSet<String> {
    correspondence_entries(dialect)
        .iter()
        .map(|(identifier, _)| (*identifier).to_owned())
        .collect()
}

#[test]
fn the_phpstan_table_covers_its_catalogue_exactly() {
    assert_eq!(
        table_keys(Dialect::Phpstan),
        catalogue("phpstan-identifiers.txt"),
        "table and catalogue must be the same set: an identifier in one \
         but not the other is an untriaged entry or a stale catalogue",
    );
}

#[test]
fn the_psalm_table_covers_its_catalogue_exactly() {
    assert_eq!(table_keys(Dialect::Psalm), catalogue("psalm-issues.txt"));
}

/// Every registry identifier no `Codes` entry of either dialect names,
/// reviewed one by one. The per-code fixture gate in `suppressions.rs`
/// is keyed off the codes *present* in the tables, so a code named
/// nowhere is invisible to it by construction: that is exactly how
/// CEL0033 stayed unreachable from both dialects after Celerrate split
/// the class-constant access site finer than either foreign tool does.
/// This list is the review record that closes that hole, and every
/// entry states why no foreign identifier maps to it.
const UNMAPPED_BY_DESIGN: &[&str] = &[
    // Decode resilience from `celerrate_db`, claimed by no rule and
    // always emitted; no foreign tool reports a size limit of ours.
    "CEL0001",
    // CEL0002 to CEL0017 are syntax and lexer resilience from
    // `celerrate_syntax`, claimed by no rule and always emitted. Both
    // foreign tools abort a file on a parse error instead of naming one
    // per malformed construct, so neither has identifiers to fold here
    // and neither has a vocabulary to borrow.
    "CEL0002", "CEL0003", "CEL0004", "CEL0005", "CEL0006", "CEL0007", "CEL0008", "CEL0009",
    "CEL0010", "CEL0011", "CEL0012", "CEL0013", "CEL0014", "CEL0015", "CEL0016", "CEL0017",
    // Syntax version gating: a construct the source uses is newer than
    // the project's configured minimum PHP version. Neither dialect
    // folds this anywhere. PHPStan's `phpstan.php` reports on its own
    // configuration, not on a gated construct at its use site, and
    // Psalm has no counterpart at all.
    "CEL0024",
    // CEL0025 to CEL0029 and CEL0039 to CEL0040 are project-anchored
    // discovery notices from `celerrate_project`: they speak about
    // composer.json, installed.json, and the resolved PHP version
    // range, not about a line of PHP, and they are counted separately
    // from diagnostics. No inline directive of any dialect can sit on
    // their anchor, so no correspondence would ever be reachable.
    "CEL0025", "CEL0026", "CEL0027", "CEL0028", "CEL0029", "CEL0039", "CEL0040",
    // CEL0041 and CEL0042 speak about `@celerrate-ignore` directives
    // themselves and fire on native directives only, so a foreign
    // identifier mapping to either would be meaningless.
    "CEL0041", "CEL0042",
    // CEL0043 to CEL0049 are anchored in `celerrate.toml` from
    // `celerrate_config`: they speak about Celerrate's own configuration
    // file, not about a line of PHP. Both dialects suppress through
    // inline PHP comments, and a TOML file has none, so no foreign
    // directive could ever sit on their anchor; neither tool has an
    // identifier for a competitor's configuration either.
    "CEL0043", "CEL0044", "CEL0045", "CEL0046", "CEL0047", "CEL0048", "CEL0049",
];

/// Every Celerrate code some `Codes` entry of either dialect names.
fn mapped_codes() -> BTreeSet<String> {
    [Dialect::Phpstan, Dialect::Psalm]
        .into_iter()
        .flat_map(correspondence_entries)
        .filter_map(|(_, mapping)| match mapping {
            ForeignMapping::Codes(codes) => Some(*codes),
            _ => None,
        })
        .flatten()
        .map(|code| (*code).to_owned())
        .collect()
}

#[test]
fn every_registry_identifier_is_either_mapped_or_allowlisted() {
    let mapped = mapped_codes();
    let unmapped: BTreeSet<String> = celerrate_diagnostics::REGISTRY
        .iter()
        .map(|entry| entry.id.as_str().to_owned())
        .filter(|code| !mapped.contains(code))
        .collect();
    let allowlisted: BTreeSet<String> = UNMAPPED_BY_DESIGN
        .iter()
        .map(|code| (*code).to_owned())
        .collect();
    assert_eq!(
        unmapped, allowlisted,
        "the registry identifiers no `Codes` entry of either dialect \
         names must equal the reviewed UNMAPPED_BY_DESIGN allowlist \
         above.\n\
         A NEW CEL identifier is a trigger to re-triage BOTH tables in \
         crates/celerrate_phpdoc_bridge/src/correspondence.rs, and so \
         is splitting an existing family finer than the foreign tools \
         do (that is how CEL0033 became unreachable from a table nobody \
         had touched).\n\
         Choose one and do it deliberately: either map the identifier \
         from every foreign identifier whose finding class covers it \
         (over-suppression is the accepted direction, under-suppression \
         is the bug), and add its arm to the per-code fixture tables in \
         crates/celerrate_cli/tests/suppressions.rs; or add it to \
         UNMAPPED_BY_DESIGN with a one-line comment saying why no \
         foreign identifier maps to it.",
    );
}

#[test]
fn every_mapped_code_is_a_registered_identifier() {
    for dialect in [Dialect::Phpstan, Dialect::Psalm] {
        for (identifier, mapping) in correspondence_entries(dialect) {
            if let ForeignMapping::Codes(codes) = mapping {
                for code in *codes {
                    assert!(
                        celerrate_diagnostics::find_identifier(code).is_some(),
                        "{dialect:?} {identifier} maps to unregistered {code}",
                    );
                }
            }
        }
    }
}
