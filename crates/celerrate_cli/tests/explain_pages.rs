//! The executable explain-page harness: every
//! written page's failing example must fire its identifier through
//! the full product pipeline, and its fixed example must not.
//! Identifiers on the declared exemption list keep the page
//! requirement but waive execution. An explain page outside the
//! exemption can neither lie nor rot.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use celerrate_cli::{ColorMode, run};
use celerrate_diagnostics::{EXECUTABLE_EXAMPLE_EXEMPTIONS, REGISTRY};

/// The manifest wrapped around a plain-snippet example. Pages that
/// need another PHP range or another file set carry their own files
/// through `//// ` markers (see `fixture_files`).
const DEFAULT_MANIFEST: &str =
    r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#;

/// Splits an example into its fixture files. A line starting with
/// `//// ` opens a new file at the path that follows; with markers
/// the file set is exactly what the example declares (so a fixture
/// without `composer.json` is expressible). Without markers the
/// whole example is one `src/Example.php` plus the default manifest.
fn fixture_files(example: &str) -> Vec<(String, String)> {
    if !example.lines().any(|line| line.starts_with("//// ")) {
        return vec![
            ("composer.json".to_string(), DEFAULT_MANIFEST.to_string()),
            ("src/Example.php".to_string(), example.to_string()),
        ];
    }
    let mut files: Vec<(String, String)> = Vec::new();
    for line in example.lines() {
        if let Some(path) = line.strip_prefix("//// ") {
            files.push((path.trim().to_string(), String::new()));
        } else if let Some((_, contents)) = files.last_mut() {
            contents.push_str(line);
            contents.push('\n');
        } else {
            panic!("example text before the first `//// ` marker: {line}");
        }
    }
    files
}

/// Forces every core rule active through the product's own channel
/// (`[rules.<name>] enabled = true`): a valid no-op for `Default`-tier
/// rules today, and the force-activation the spec requires the day the
/// first nursery rule lands, with no harness change. Skipped when the
/// example declares its own `celerrate.toml`.
fn force_activation_configuration() -> String {
    let mut text = String::new();
    for (metadata, _) in celerrate_rules::core_rules() {
        text.push_str("[rules.");
        text.push_str(&metadata.name);
        text.push_str("]\nenabled = true\n\n");
    }
    text
}

/// Runs `celerrate check` over the example's fixture and returns the
/// full plain-color report.
fn report_for(example: &str) -> String {
    let root = tempfile::tempdir().unwrap();
    let files = fixture_files(example);
    let declares_configuration = files.iter().any(|(path, _)| path == "celerrate.toml");
    for (path, contents) in files {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
    if !declares_configuration {
        std::fs::write(
            root.path().join("celerrate.toml"),
            force_activation_configuration(),
        )
        .unwrap();
    }
    let mut output = Vec::new();
    run(
        vec![
            "celerrate".into(),
            "check".into(),
            root.path().as_os_str().into(),
        ],
        &mut output,
        ColorMode::Plain,
    );
    String::from_utf8(output).unwrap()
}

#[test]
fn every_written_page_example_is_honest() {
    let mut failures = Vec::new();
    for entry in REGISTRY {
        let page = entry.explain;
        if EXECUTABLE_EXAMPLE_EXEMPTIONS
            .iter()
            .any(|exemption| exemption.id == entry.id)
        {
            continue;
        }
        let identifier = entry.id.as_str();
        let failing = report_for(page.failing_example);
        if !failing.contains(identifier) {
            failures.push(format!(
                "{identifier}: the failing example does not fire it:\n{failing}"
            ));
        }
        let fixed = report_for(page.fixed_example);
        if fixed.contains(identifier) {
            failures.push(format!(
                "{identifier}: the fixed example still fires it:\n{fixed}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
}

/// Explain pages stay executable for every tier, because the harness
/// forces each rule
/// active through the same `[rules]` mechanism a user would. This
/// pins that the generated configuration names every core rule and is
/// itself silent, so no page's report can be polluted by it.
#[test]
fn the_forced_activation_names_every_core_rule_and_stays_silent() {
    let configuration = force_activation_configuration();
    for (metadata, _) in celerrate_rules::core_rules() {
        assert!(
            configuration.contains(&format!("[rules.{}]", metadata.name)),
            "rule `{}` is missing from the forced activation",
            metadata.name,
        );
    }
    let report = report_for("<?php\n\nfunction example(): void {}\n");
    // Named rather than prefix-matched: a bare "CEL004" prefix also
    // catches CEL0041 and CEL0042, which are legitimate reporting-phase
    // rule output, not configuration diagnostics.
    for identifier in [
        "CEL0043", "CEL0044", "CEL0045", "CEL0046", "CEL0047", "CEL0048", "CEL0049", "CEL0050",
        "CEL0051",
    ] {
        assert!(
            !report.contains(identifier),
            "the forced activation must not report configuration diagnostics:\n{report}",
        );
    }
}
