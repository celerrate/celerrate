//! Colorless rendering snapshots (design section 11): the rich block
//! shapes, pinned byte for byte in `ColorMode::Plain`.

#![cfg(feature = "render")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_diagnostics::{Diagnostic, Severity, find_identifier};
use celerrate_rules::render::{
    ColorMode, DegradeEverything, FaultInjection, SourceAccess, render_report,
};
use celerrate_source::{FileId, TextRange, TextSize};

struct FixtureSources(Vec<(FileId, &'static str, &'static str)>);

impl SourceAccess for FixtureSources {
    fn display_path(&self, file: FileId) -> Option<String> {
        self.0
            .iter()
            .find(|(id, _, _)| *id == file)
            .map(|(_, path, _)| (*path).to_owned())
    }

    fn text(&self, file: FileId) -> Option<&str> {
        self.0
            .iter()
            .find(|(id, _, _)| *id == file)
            .map(|(_, _, text)| *text)
    }
}

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::from(start), TextSize::from(end))
}

const KERNEL: &str = "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n";

fn kernel_diagnostic() -> Diagnostic {
    Diagnostic::spanned(
        find_identifier("CEL0018").unwrap(),
        Severity::Error,
        FileId::new(0),
        range(43, 50),
        "unknown class `Missing`".to_owned(),
    )
}

fn sources() -> FixtureSources {
    FixtureSources(vec![(FileId::new(0), "src/Kernel.php", KERNEL)])
}

#[test]
fn a_span_diagnostic_renders_a_rustc_style_block() {
    let report = render_report(
        &[kernel_diagnostic()],
        &sources(),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("single_span", report.blocks.join("\n\n"));
}

const CALLER: &str =
    "<?php\nnamespace App;\n\nfunction render(User $user): void\n{\n    $user->svae();\n}\n";

#[test]
fn labels_notes_and_suggestions_render_in_the_block() {
    use celerrate_diagnostics::{Confidence, Label, LabelTarget, Suggestion};
    use celerrate_source::TextEdit;

    let file = FileId::new(0);
    // `svae` in `$user->svae();` — verified against CALLER with
    // `CALLER.find("svae")`.
    let member = range(69, 73);
    let mut diagnostic = Diagnostic::spanned(
        find_identifier("CEL0030").unwrap(),
        Severity::Error,
        file,
        member,
        "unknown method `svae` on `App\\User`".to_owned(),
    );
    diagnostic.labels.push(Label {
        // `User $user` in the signature — the receiver's declared type,
        // verified against CALLER with `CALLER.find("User $user")`.
        target: LabelTarget::Local {
            range: range(38, 42),
        },
        message: "the receiver is typed `App\\User` here".to_owned(),
    });
    diagnostic
        .notes
        .push("`App\\User` declares no method or magic accessor named `svae`".to_owned());
    diagnostic.suggestions.push(Suggestion {
        message: "did you mean `save`?".to_owned(),
        confidence: Confidence::NeedsReview,
        edits: vec![TextEdit {
            file,
            range: member,
            replacement: "save".to_owned(),
        }],
    });

    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![(file, "src/Caller.php", CALLER)]),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("anatomy", report.blocks.join("\n\n"));
}

#[test]
fn unicode_content_renders_without_column_drift() {
    // Multi-byte content before and inside the underlined range: the
    // caret must sit under the token, not at its byte offset.
    let source = "<?php\n// café ☕\n$noël = strlen_typo(\"été\");\n";
    let file = FileId::new(0);
    let start = source.find("strlen_typo").unwrap() as u32;
    let diagnostic = Diagnostic::spanned(
        find_identifier("CEL0019").unwrap(),
        Severity::Error,
        file,
        range(start, start + 11),
        "unknown function `strlen_typo`".to_owned(),
    );
    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![(file, "src/unicode.php", source)]),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("unicode", report.blocks.join("\n\n"));
}

#[test]
fn a_project_diagnostic_keeps_the_notice_vocabulary_without_an_excerpt() {
    let diagnostic = Diagnostic::project(
        find_identifier("CEL0025").unwrap(),
        Severity::Warning,
        "no composer.json found; analyzing the whole project root".to_owned(),
    );
    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![]),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    assert_eq!(
        report.blocks,
        vec!["notice CEL0025: no composer.json found; analyzing the whole project root".to_owned()],
    );
}
