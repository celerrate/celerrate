//! The hidden `ground-truth` channel (design section 10, harness 1):
//! its record format is a contract `cargo xtask ground-truth` (task
//! 12) pins against a committed baseline, so these tests drive it
//! exactly as that consumer will, through `celerrate_cli::run`.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;

#[test]
fn divergences_print_sorted_with_the_summary_line() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("code.php"),
        r#"<?php
namespace App;
/** @return string */
function wrong() { return 1; }
/** @return int */
function right() { return 1; }
class Holder {
    /** @return string */
    public function alsoWrong() { return 2; }
}
"#,
    )
    .unwrap();
    let mut output = Vec::new();
    let outcome = celerrate_cli::run(
        vec![
            "celerrate".into(),
            "ground-truth".into(),
            project.path().as_os_str().to_owned(),
        ],
        &mut output,
        celerrate_cli::ColorMode::Plain,
    );
    let printed = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(
        lines,
        [
            "app\\holder::alsowrong\t2\tstring",
            "app\\wrong\t1\tstring",
            "checked 3, divergences 2",
        ],
        "sorted records, then the summary; the compatible function is silent",
    );
    assert_eq!(outcome, celerrate_cli::Outcome::Clean);
}

#[test]
fn bodiless_and_unannotated_members_are_not_checked() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("code.php"),
        r#"<?php
namespace App;
interface Contract {
    /** @return string */
    public function declared(): string;
}
function unannotated() { return 1; }
"#,
    )
    .unwrap();
    let mut output = Vec::new();
    let _ = celerrate_cli::run(
        vec![
            "celerrate".into(),
            "ground-truth".into(),
            project.path().as_os_str().to_owned(),
        ],
        &mut output,
        celerrate_cli::ColorMode::Plain,
    );
    let printed = String::from_utf8(output).unwrap();
    assert_eq!(printed.lines().last(), Some("checked 0, divergences 0"));
}

/// The compatibility relation between a genuinely unresolvable
/// annotation and any inferred return is `Proof::CannotProve`, never
/// `Proof::Fails`: a class-level `@template T` with no call-site
/// argument to bind it is undecidable, not refuted, and decision 13
/// requires it to pass in silence (inference-only generics make
/// precision asymmetric by design). Without this test, swapping the
/// harness's `Proof::Fails` check for "anything but `Holds`" would
/// flood the report and nothing here would catch it.
#[test]
fn a_template_annotated_return_cannot_be_proven_and_passes_in_silence() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("code.php"),
        r#"<?php
namespace App;
/** @template T */
class Box {
    /** @return T */
    public function get() { return "definitely not T"; }
}
"#,
    )
    .unwrap();
    let mut output = Vec::new();
    let _ = celerrate_cli::run(
        vec![
            "celerrate".into(),
            "ground-truth".into(),
            project.path().as_os_str().to_owned(),
        ],
        &mut output,
        celerrate_cli::ColorMode::Plain,
    );
    let printed = String::from_utf8(output).unwrap();
    assert_eq!(
        printed.lines().collect::<Vec<_>>(),
        ["checked 1, divergences 0"],
        "an inferred string against an unbound template is undecidable, not refuted: {printed}",
    );
}

/// `display.rs` renders a literal string with no escaping
/// (`format!("'{value}'")`), so a divergent function whose inferred
/// return is a string literal containing a raw tab and a raw newline
/// must still yield exactly one record line with exactly three
/// tab-separated fields: the escaping has to happen at the record
/// boundary in `ground_truth.rs`, since `display.rs` is shared
/// rendering this plan may not change.
///
/// The single-quoted PHP literal below embeds a genuine tab byte and a
/// genuine newline byte between its quotes (PHP does not interpret
/// `\t`/`\n` escapes in single-quoted strings, so the only way to get
/// real control characters into a literal-string type is to place the
/// real bytes in the source, which this non-raw Rust string literal
/// does via its own `\t`/`\n` escapes).
#[test]
fn a_literal_containing_a_tab_and_a_newline_stays_one_record_one_line() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("code.php"),
        "<?php\nnamespace App;\n/** @return int */\nfunction withControlCharacters() { return 'a\tb\nc'; }\n",
    )
    .unwrap();
    let mut output = Vec::new();
    let outcome = celerrate_cli::run(
        vec![
            "celerrate".into(),
            "ground-truth".into(),
            project.path().as_os_str().to_owned(),
        ],
        &mut output,
        celerrate_cli::ColorMode::Plain,
    );
    let printed = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(
        lines,
        [
            "app\\withcontrolcharacters\t'a\\tb\\nc'\tint",
            "checked 1, divergences 1",
        ],
        "the raw tab and newline in the literal must be escaped, not left \
         to split the record across lines or fields: {printed:?}",
    );
    assert_eq!(outcome, celerrate_cli::Outcome::Clean);
}

#[test]
fn the_subcommand_is_hidden_from_help() {
    let mut output = Vec::new();
    let _ = celerrate_cli::run(
        vec!["celerrate".into(), "--help".into()],
        &mut output,
        celerrate_cli::ColorMode::Plain,
    );
    let printed = String::from_utf8(output).unwrap();
    assert!(
        !printed.contains("ground-truth"),
        "internal channel, plan 9c owns the product surface: {printed}",
    );
}
