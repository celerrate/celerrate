//! The emission-side half of the "no check family outside the
//! framework" gate (design sections 2 and 5). The allocation ledger
//! (`celerrate_cli/tests/registry.rs`) constrains declarations; this
//! scan constrains emissions: a domain crate whose families migrated
//! to `celerrate_rules` must not construct `Diagnostic` values
//! anywhere under `src/`, so a check family cannot quietly grow back
//! below the framework. Resilience producers (`celerrate_db`,
//! `celerrate_syntax`, `celerrate_project`) are not governed: their
//! diagnostics are neither disableable nor configurable by nature and
//! stay produced by their crates.
//!
//! This is a literal string scan over file contents, not a parser: it
//! catches the spellings named in `FORBIDDEN_CALLS` and nothing else.
//! `Diagnostic::spanned(` and `Diagnostic::project(` are `Diagnostic`'s
//! only associated constructors today, but its fields are public, so a
//! struct literal (`Diagnostic { id, severity, .. }`, including
//! struct-update syntax against an existing value) builds one without
//! naming either function. `"Diagnostic {"` is included to close that
//! route. That spelling is deliberately coarse: it also matches a bare
//! mention of the type immediately followed by a brace, for instance a
//! function whose return type is `Diagnostic` (`-> Diagnostic {`), or,
//! in principle, a `match`/`let` pattern that destructures one. None of
//! those currently occur in a governed crate. `celerrate_semantics`
//! does name `celerrate_diagnostics` now, for identifier vocabulary
//! (`DiagnosticId`, `find_identifier`) its suppression-filter
//! resolution needs — the `Diagnostic` value model itself stays
//! unnamed in either governed crate's `src/`, which is the point of
//! the migration these crates went through. Should the value model
//! ever appear, it is a legitimate reason for this gate to ask a human
//! to look, since a governed crate has no business naming the
//! diagnostic model at all, constructing or not. Anything this scan
//! cannot see (reflection,
//! macro-generated construction, a helper crate re-exporting a wrapping
//! function under another name) is out of scope; it works on source
//! text, the same technique `registry.rs` uses for declarations.

use std::path::{Path, PathBuf};

/// The crates that must not construct diagnostics. Grows when another
/// domain crate's families migrate.
const GOVERNED_CRATES: &[&str] = &["celerrate_semantics", "celerrate_types"];

/// The construction surface of the shared model: the two named
/// constructors, plus the struct-literal spelling that bypasses them
/// (see the module doc). A new constructor on `Diagnostic` must be
/// added here or the gate silently narrows.
const FORBIDDEN_CALLS: &[&str] = &[
    "Diagnostic::spanned(",
    "Diagnostic::project(",
    "Diagnostic {",
];

pub fn run() -> crate::Result<()> {
    let root = crate::workspace_root()?;
    for crate_name in GOVERNED_CRATES {
        let sources = root.join("crates").join(crate_name).join("src");
        if !sources.is_dir() {
            return Err(format!(
                "emission scan: {crate_name} has no src/ directory (renamed, or the scan broke)"
            )
            .into());
        }
        if let Some((file, call)) = first_emission_in(&sources) {
            return Err(format!(
                "emission gate violated: {} constructs a diagnostic ({call} in {}); \
                 check families construct diagnostics in celerrate_rules, domain crates \
                 produce outcomes and records only",
                crate_name,
                file.display(),
            )
            .into());
        }
    }
    Ok(())
}

/// The first forbidden construction under `directory`, depth-first.
fn first_emission_in(directory: &Path) -> Option<(PathBuf, &'static str)> {
    let entries = std::fs::read_dir(directory).ok()?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            if let Some(found) = first_emission_in(&path) {
                return Some(found);
            }
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        for call in FORBIDDEN_CALLS {
            if source.contains(call) {
                return Some((path, call));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::first_emission_in;

    #[test]
    fn a_clean_tree_scans_to_none() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("walk.rs"),
            "pub fn outcomes() -> Vec<ReferenceOutcome> { Vec::new() }\n",
        )
        .unwrap();
        assert!(first_emission_in(directory.path()).is_none());
    }

    #[test]
    fn a_buried_construction_is_found_with_its_call() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("checks");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("mod.rs"),
            "let d = Diagnostic::spanned(id, severity, file, range, message);\n",
        )
        .unwrap();
        let (file, call) = first_emission_in(directory.path()).unwrap();
        assert!(file.ends_with("checks/mod.rs"));
        assert_eq!(call, "Diagnostic::spanned(");
    }

    #[test]
    fn the_project_anchored_constructor_is_governed_too() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("notice.rs"),
            "Diagnostic::project(id, severity, message)\n",
        )
        .unwrap();
        assert!(first_emission_in(directory.path()).is_some());
    }

    #[test]
    fn a_struct_literal_construction_bypassing_both_constructors_is_found() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("bypass.rs"),
            "let d = Diagnostic { id, severity, anchor, message, labels: Vec::new(), notes: Vec::new(), suggestions: Vec::new() };\n",
        )
        .unwrap();
        let (file, call) = first_emission_in(directory.path()).unwrap();
        assert!(file.ends_with("bypass.rs"));
        assert_eq!(call, "Diagnostic {");
    }
}
