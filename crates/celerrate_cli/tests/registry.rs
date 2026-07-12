//! The uniqueness test. It lives here and nowhere else: in a strict
//! dependency DAG the composition root is the only place that can observe
//! every producer at once, which is exactly why `celerrate_project` and
//! `celerrate_semantics` could both allocate `CEL0018` with two passing
//! stability tests. A seventh crate cannot repeat that mistake without
//! failing this.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::BTreeMap;

use celerrate_diagnostics::{DiagnosticId, REGISTRY};

/// Every producer, named, with what it says it allocates.
fn producers() -> Vec<(&'static str, &'static [DiagnosticId])> {
    vec![
        ("celerrate_db", celerrate_db::ALLOCATED_IDENTIFIERS),
        ("celerrate_syntax", celerrate_syntax::ALLOCATED_IDENTIFIERS),
        (
            "celerrate_semantics",
            celerrate_semantics::ALLOCATED_IDENTIFIERS,
        ),
        (
            "celerrate_project",
            celerrate_project::ALLOCATED_IDENTIFIERS,
        ),
    ]
}

#[test]
fn every_identifier_is_allocated_by_exactly_one_producer() {
    let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (producer, allocated) in producers() {
        for id in allocated {
            owners.entry(id.as_str()).or_default().push(producer);
        }
    }
    let collisions: Vec<(&str, Vec<&str>)> = owners
        .iter()
        .filter(|(_, producers)| producers.len() > 1)
        .map(|(id, producers)| (*id, producers.clone()))
        .collect();
    assert!(
        collisions.is_empty(),
        "an identifier means one thing, forever: {collisions:?}",
    );
}

#[test]
fn the_registry_is_exactly_what_the_producers_allocate() {
    let mut allocated: Vec<&str> = producers()
        .iter()
        .flat_map(|(_, identifiers)| identifiers.iter())
        .map(DiagnosticId::as_str)
        .collect();
    allocated.sort_unstable();

    let mut registered: Vec<&str> = REGISTRY.iter().map(|entry| entry.id.as_str()).collect();
    registered.sort_unstable();

    assert_eq!(
        allocated, registered,
        "the registry and the producers must not drift: add the identifier \
         to `celerrate_diagnostics::REGISTRY` when you allocate it",
    );
}

#[test]
fn the_registry_names_the_producer_that_actually_allocates() {
    for (producer, allocated) in producers() {
        for id in allocated {
            let entry = REGISTRY
                .iter()
                .find(|entry| entry.id == *id)
                .unwrap_or_else(|| panic!("{} is not in the registry", id.as_str()));
            assert_eq!(
                entry.owner,
                producer,
                "{} is allocated by {producer} but the registry credits {}",
                id.as_str(),
                entry.owner,
            );
        }
    }
}
