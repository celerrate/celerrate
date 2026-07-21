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
