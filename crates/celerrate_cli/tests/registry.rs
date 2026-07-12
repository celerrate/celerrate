//! The uniqueness test. It lives here and nowhere else: in a strict
//! dependency DAG the composition root is the only place that can observe
//! every producer at once, which is exactly why `celerrate_project` and
//! `celerrate_semantics` could both allocate `CEL0018` with two passing
//! stability tests. A seventh crate cannot repeat that mistake without
//! failing this.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use celerrate_diagnostics::{DiagnosticId, REGISTRY};

/// Every producer, named, with what it says it allocates.
///
/// Hand-maintained, and therefore not trusted:
/// `every_producer_crate_the_composition_root_depends_on_is_named_above`
/// checks this list against the dependency graph.
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

/// The hole the rest of this file could not see.
///
/// `producers()` above and `celerrate_diagnostics::REGISTRY` are both
/// written by hand, and every check here reads only what they name. A new
/// producer crate omitted from both is therefore invisible to the very
/// collision check this file exists to provide: it could allocate an
/// identifier another crate already owns, and all three tests above would
/// still pass, which is exactly the failure that let `celerrate_project`
/// and `celerrate_semantics` both allocate `CEL0018`.
///
/// So the list is checked against the dependency graph rather than
/// believed. Every `celerrate_*` crate the composition root depends on
/// (dev-dependencies included: `celerrate_syntax` is reached through one)
/// that exports an `ALLOCATED_IDENTIFIERS` is a producer by that fact, and
/// must be named above. The constant is the signal because it *is* the
/// allocation: a crate that has one can collide, and a crate that has none
/// cannot.
///
/// Reading the filesystem here is fine. This is a test, not a query.
#[test]
fn every_producer_crate_the_composition_root_depends_on_is_named_above() {
    let named: BTreeSet<&str> = producers().iter().map(|(name, _)| *name).collect();
    let missing: Vec<String> = celerrate_dependencies()
        .into_iter()
        .filter(|dependency| allocates_identifiers(dependency))
        .filter(|dependency| !named.contains(dependency.as_str()))
        .collect();

    assert!(
        missing.is_empty(),
        "these crates allocate diagnostic identifiers, and nothing checks them for collisions \
         with the ones already taken.\nAdd each to `producers()` in this file, and add every \
         identifier it allocates to `celerrate_diagnostics::REGISTRY`: {missing:?}",
    );
}

/// Every `celerrate_*` path dependency of the composition root, read from
/// its own manifest at test time: dependencies and dev-dependencies alike.
fn celerrate_dependencies() -> Vec<String> {
    let manifest = std::fs::read_to_string(composition_root().join("Cargo.toml")).unwrap();
    manifest
        .lines()
        .filter_map(|line| line.split_once(" = "))
        .map(|(name, _)| name.trim())
        .filter(|name| name.starts_with("celerrate_"))
        .map(str::to_owned)
        .collect()
}

/// Whether a crate exports an `ALLOCATED_IDENTIFIERS`, which is what makes
/// it a producer.
fn allocates_identifiers(dependency: &str) -> bool {
    let library = composition_root()
        .parent()
        .unwrap()
        .join(dependency)
        .join("src/lib.rs");
    std::fs::read_to_string(library)
        .map(|source| source.contains("ALLOCATED_IDENTIFIERS"))
        .unwrap_or(false)
}

fn composition_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
