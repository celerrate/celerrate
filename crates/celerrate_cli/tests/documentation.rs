//! Documentation drift tests. The repository documentation is the
//! interim publication home for the identifier reference and the
//! bridge tables (type-engine design, section 9); these tests live at
//! the composition root for the same reason the identifier-uniqueness
//! test does: it is the only layer that sees every producer at once.
//! A plan that allocates a new identifier without documenting it
//! fails here.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;

use celerrate_diagnostics::REGISTRY;

fn workspace_page(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is unreadable: {error}", path.display()))
}

#[test]
fn every_registered_identifier_is_documented() {
    let page = workspace_page("docs/diagnostics.md");
    for entry in REGISTRY {
        assert!(
            page.contains(entry.id.as_str()),
            "docs/diagnostics.md does not document {} (`{}`)",
            entry.id.as_str(),
            entry.family,
        );
    }
}
