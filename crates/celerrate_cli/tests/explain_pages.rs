//! The executable explain-page harness (design section 10): every
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

/// Runs `celerrate check` over the example's fixture and returns the
/// full plain-color report.
fn report_for(example: &str) -> String {
    let root = tempfile::tempdir().unwrap();
    for (path, contents) in fixture_files(example) {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
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

/// The spec's harness forces nursery rules active before running
/// their pages; no nursery rule exists, so that machinery does not
/// either. This guard fails the moment the first nursery rule lands,
/// naming the extension required.
#[test]
fn every_core_rule_is_default_tier_so_the_default_active_set_covers_all_pages() {
    for (metadata, _) in celerrate_rules::core_rules() {
        assert_eq!(
            metadata.tier,
            celerrate_rules::Tier::Default,
            "rule `{}` is outside the default active set; teach \
             explain_pages.rs to force it active before its \
             identifiers' pages can stay executable",
            metadata.name,
        );
    }
}
