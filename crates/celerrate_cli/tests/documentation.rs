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

#[test]
fn every_output_format_is_documented() {
    let page = workspace_page("docs/output-formats.md");
    for format in ["human", "json", "sarif", "github"] {
        assert!(
            page.contains(&format!("`{format}`")),
            "docs/output-formats.md does not document `{format}`",
        );
    }
    assert!(page.contains("schema_version"));
    assert!(page.contains("schemas/celerrate-json-report.v1.schema.json"));
    // The following assertions protect the parts of the page most likely to
    // drift: the internal-error detail that travels in every machine
    // format, and the two extra fields the SARIF writer carries beyond the
    // shared model.
    for detail in [
        "internal_errors",
        "toolExecutionNotifications",
        "::error::",
        "baselinedHidden",
        "properties.rule",
        "tool.driver.notifications",
    ] {
        assert!(
            page.contains(detail),
            "docs/output-formats.md does not document `{detail}`",
        );
    }
}

#[test]
fn the_bridge_page_documents_every_suppression_form() {
    let page = workspace_page("docs/phpdoc-bridge.md");
    for form in [
        "@phpstan-ignore-line",
        "@phpstan-ignore-next-line",
        "@phpstan-ignore",
        "@psalm-suppress",
    ] {
        assert!(
            page.contains(form),
            "docs/phpdoc-bridge.md does not document `{form}`",
        );
    }
}
