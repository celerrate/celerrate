//! Negative-proof guard for the substitution funnel (issue #39):
//! `Walker::member_boundary_type` in `src/flow/boundary.rs` is the
//! one funnel every member read, method call, callable projection,
//! and `new` result passes through. The call sites outside it are
//! deliberate and enumerated below — the PR #35 whole-branch review
//! verified this list by hand; this test keeps it verified by
//! machine. An unlisted site is not necessarily a bug, but it is
//! necessarily a decision: justify it against the invariant in
//! `src/flow/boundary.rs`'s rustdoc, then add it here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;

/// Production call sites of `substitution::substitute`, by file
/// relative to `src/`, with their exact count. `substitution.rs`
/// itself is exempt (its own recursion and its test module).
const ALLOWED_CALL_SITES: &[(&str, usize)] = &[
    ("declared.rs", 2),      // written-type substitution at declaration reading
    ("flow/boundary.rs", 1), // member_boundary_type — THE funnel
    ("flow/calls.rs", 1),    // solved_call_result — call-site template solving
    ("inheritance.rs", 2),   // parent-argument substitution along linearization
    ("solver.rs", 2),        // template-map application and bound checking
];

#[test]
fn substitution_stays_funneled_through_member_boundary_type() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeMap::new();
    collect_call_sites(&source_root, &source_root, &mut found);

    let expected: BTreeMap<String, usize> = ALLOWED_CALL_SITES
        .iter()
        .map(|(file, count)| ((*file).to_owned(), *count))
        .collect();

    assert_eq!(
        found, expected,
        "substitution call sites changed. `member_boundary_type` in \
         src/flow/boundary.rs is the one funnel every member read, \
         method call, callable projection, and `new` result passes \
         through (issue #39). A new `substitute` call site must be \
         justified against that invariant and, if legitimate, added \
         to ALLOWED_CALL_SITES with its count."
    );
}

/// Counts non-comment lines containing a `substitute(` call per file
/// under `src/`, excluding `substitution.rs` (the defining module)
/// and `fn substitute` definitions. Textual on purpose: the guard
/// must catch call sites regardless of import style
/// (`crate::substitution::substitute(...)` or a `use`d bare
/// `substitute(...)`).
fn collect_call_sites(root: &Path, directory: &Path, found: &mut BTreeMap<String, usize>) {
    for entry in std::fs::read_dir(directory).expect("source directory is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() {
            collect_call_sites(root, &path, found);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("file is under src")
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "substitution.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("source file is readable");
        let count = text
            .lines()
            .map(str::trim_start)
            .filter(|line| !line.starts_with("//"))
            .filter(|line| line.contains("substitute("))
            .filter(|line| !line.contains("fn substitute"))
            .count();
        if count > 0 {
            found.insert(relative, count);
        }
    }
}
