//! The uniqueness test. It lives here and nowhere else: in a strict
//! dependency DAG the composition root is the only place that can observe
//! every producer at once, which is exactly why `celerrate_project` and
//! `celerrate_semantics` could both allocate `CEL0018` with two passing
//! stability tests. A seventh crate cannot repeat that mistake without
//! failing this.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use celerrate_diagnostics::{DiagnosticId, REGISTRY};

/// Every producer, named, with what it says it allocates.
///
/// Hand-maintained, and therefore not trusted:
/// `the_named_producers_are_exactly_the_producers_in_the_dependency_graph`
/// checks this list against the dependency graph.
fn producers() -> Vec<(&'static str, &'static [DiagnosticId])> {
    vec![
        ("celerrate_config", celerrate_config::ALLOCATED_IDENTIFIERS),
        ("celerrate_db", celerrate_db::ALLOCATED_IDENTIFIERS),
        ("celerrate_syntax", celerrate_syntax::ALLOCATED_IDENTIFIERS),
        (
            "celerrate_project",
            celerrate_project::ALLOCATED_IDENTIFIERS,
        ),
        ("celerrate_rules", celerrate_rules::ALLOCATED_IDENTIFIERS),
        (
            "celerrate_cli",
            celerrate_cli::baseline::ALLOCATED_IDENTIFIERS,
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
/// The derivation is asserted **equal** to the list, in both directions,
/// rather than merely a subset of it. A derivation that reads a manifest
/// and scans a source tree can break, and a broken one that finds nothing
/// would satisfy any one-directional check while checking nothing at all,
/// which is the very hole this test exists to close. Equality makes a
/// broken derivation fail as loudly as a missing producer.
///
/// Reading the filesystem here is fine. This is a test, not a query.
#[test]
fn the_named_producers_are_exactly_the_producers_in_the_dependency_graph() {
    let named: BTreeSet<String> = producers()
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    let mut derived: BTreeSet<String> = celerrate_dependencies()
        .into_iter()
        .filter(|dependency| allocates_identifiers(&crate_directory(dependency)))
        .collect();
    // The composition root itself allocates the baseline notices, because
    // the baseline mechanics live here rather than in a dependency: the
    // scan above only ever looks at what `celerrate_cli` depends on, so it
    // must be added to the derived set by hand.
    derived.insert("celerrate_cli".to_string());

    let unnamed: Vec<&String> = derived.difference(&named).collect();
    let underived: Vec<&String> = named.difference(&derived).collect();

    assert_eq!(
        derived, named,
        "the dependency graph and `producers()` disagree.\n\
         derived from the graph: {derived:?}\n\
         named in `producers()`: {named:?}\n\
         \n\
         allocate identifiers but are not named in `producers()`: {unnamed:?}\n\
         Add each to `producers()` in this file, and add every identifier it allocates to \
         `celerrate_diagnostics::REGISTRY`. Until then nothing checks it for collisions with \
         the identifiers already taken.\n\
         \n\
         named in `producers()` but not found in the graph: {underived:?}\n\
         The derivation reads the composition root's manifest for its `celerrate_*` \
         dependencies and scans each one's `src/` for `ALLOCATED_IDENTIFIERS`. If the crate \
         still allocates, the derivation is broken and must be repaired here: a derivation \
         that finds nothing would let the next producer through unchecked. If the crate no \
         longer allocates, remove it from `producers()`.",
    );
}

/// Every `celerrate_*` path dependency of the composition root, read from
/// its own manifest at test time: dependencies and dev-dependencies alike.
fn celerrate_dependencies() -> BTreeSet<String> {
    let manifest = std::fs::read_to_string(composition_root().join("Cargo.toml")).unwrap();
    dependencies_in(&manifest)
}

/// The `celerrate_*` dependencies a manifest declares, in whichever of the
/// two spellings TOML allows for the key: `celerrate_db = { path = ... }`
/// and `celerrate_db.workspace = true` name the same crate, and an
/// ordinary reformat from one to the other must not quietly shrink the
/// derived set.
fn dependencies_in(manifest: &str) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    let mut in_dependency_table = false;
    for line in manifest.lines() {
        let line = line.trim();
        if let Some(table) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            in_dependency_table = matches!(table, "dependencies" | "dev-dependencies");
            continue;
        }
        if !in_dependency_table {
            continue;
        }
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let name = key.trim().split('.').next().unwrap_or_default().trim();
        if name.starts_with("celerrate_") {
            dependencies.insert(name.to_owned());
        }
    }
    dependencies
}

/// Whether a crate allocates diagnostic identifiers, which is what makes
/// it a producer.
///
/// The whole of `src/` is scanned, not `src/lib.rs` alone: a crate is free
/// to declare its `ALLOCATED_IDENTIFIERS` in a submodule and re-export it,
/// and a scan that reads only the root module would call such a crate a
/// non-producer and stop watching it.
fn allocates_identifiers(crate_directory: &Path) -> bool {
    mentions_allocated_identifiers(&crate_directory.join("src"))
}

fn mentions_allocated_identifiers(directory: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => mentions_allocated_identifiers(&path),
            Ok(kind) if kind.is_file() => std::fs::read_to_string(&path)
                .is_ok_and(|source| source.contains("ALLOCATED_IDENTIFIERS")),
            _ => false,
        }
    })
}

fn crate_directory(name: &str) -> PathBuf {
    workspace_crates().join(name)
}

fn workspace_crates() -> PathBuf {
    composition_root()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn composition_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn a_dependency_is_read_in_either_spelling_the_manifest_may_use() {
    // `celerrate_db.workspace = true` is an ordinary reformat away, and a
    // parser that reads the key as `celerrate_db.workspace` finds no such
    // crate and silently drops the producer.
    let manifest = "\
[package]
name = \"celerrate_cli\"

[dependencies]
celerrate_db = { path = \"../celerrate_db\" }
celerrate_project.workspace = true
salsa = { workspace = true }

[dev-dependencies]
celerrate_syntax = { path = \"../celerrate_syntax\" }
";
    let expected: BTreeSet<String> = ["celerrate_db", "celerrate_project", "celerrate_syntax"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(dependencies_in(manifest), expected);
}

#[test]
fn a_producer_that_declares_its_identifiers_in_a_submodule_is_still_a_producer() {
    // A glob re-export leaves no `ALLOCATED_IDENTIFIERS` in `lib.rs`, and a
    // scan that reads only `lib.rs` would stop watching the crate.
    let crate_directory = tempfile::tempdir().unwrap();
    let sources = crate_directory.path().join("src");
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::write(
        sources.join("lib.rs"),
        "mod identifiers;\npub use identifiers::*;\n",
    )
    .unwrap();
    assert!(!allocates_identifiers(crate_directory.path()));

    std::fs::write(
        sources.join("identifiers.rs"),
        "pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[];\n",
    )
    .unwrap();
    assert!(allocates_identifiers(crate_directory.path()));
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
