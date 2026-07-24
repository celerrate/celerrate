//! `celerrate explain` end to end: a known identifier prints its
//! page, lookup is case-insensitive, an unknown identifier is a
//! usage error, and every registered identifier renders.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use celerrate_cli::{ColorMode, Outcome, run};

fn explain(identifier: &str) -> (Outcome, String) {
    let mut output = Vec::new();
    let outcome = run(
        vec!["celerrate".into(), "explain".into(), identifier.into()],
        &mut output,
        ColorMode::Plain,
    );
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn a_known_identifier_prints_its_page() {
    let (outcome, output) = explain("CEL0018");
    assert_eq!(outcome, Outcome::Clean);
    insta::assert_snapshot!(output);
}

#[test]
fn lookup_is_case_insensitive() {
    let (outcome, output) = explain("cel0018");
    assert_eq!(outcome, Outcome::Clean);
    assert!(output.starts_with("CEL0018: unknown class"));
}

#[test]
fn an_unknown_identifier_is_a_usage_error() {
    let (outcome, output) = explain("CEL9999");
    assert_eq!(outcome, Outcome::UsageError);
    assert!(output.contains("unknown diagnostic identifier `CEL9999`"));
    assert!(output.contains("identifiers look like CEL0030"));
}

#[test]
fn every_registered_identifier_prints_a_page() {
    for entry in celerrate_diagnostics::REGISTRY {
        let (outcome, output) = explain(entry.id.as_str());
        assert_eq!(outcome, Outcome::Clean, "{}", entry.id.as_str());
        assert!(
            output.starts_with(entry.id.as_str()),
            "{} page must open with its identifier",
            entry.id.as_str(),
        );
    }
}
