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
    ColorMode, DegradeEverything, FaultInjection, ResolvedLabel, SourceAccess, SymbolResolver,
    render_report,
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

/// A [`SymbolResolver`] scripted with one [`ResolvedLabel`] per symbol,
/// for tests that need to force a specific `LabelTarget::Symbolic` arm.
/// Unscripted symbols degrade, matching [`DegradeEverything`]'s default.
struct ScriptedResolver(Vec<(&'static str, ResolvedLabel)>);

impl SymbolResolver for ScriptedResolver {
    fn resolve(&self, symbol: &str) -> ResolvedLabel {
        self.0
            .iter()
            .find(|(scripted, _)| *scripted == symbol)
            .map(|(_, resolved)| *resolved)
            .unwrap_or(ResolvedLabel::Degraded)
    }
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
    // `svae` in `$user->svae();`, computed at runtime the way the
    // unicode test below does — no hardcoded byte offset to drift.
    let member_start = CALLER.find("svae").unwrap() as u32;
    let member = range(member_start, member_start + 4);
    let mut diagnostic = Diagnostic::spanned(
        find_identifier("CEL0030").unwrap(),
        Severity::Error,
        file,
        member,
        "unknown method `svae` on `App\\User`".to_owned(),
    );
    // `User` in `User $user` — the receiver's declared type in the
    // signature, computed the same way.
    let type_start = CALLER.find("User $user").unwrap() as u32;
    diagnostic.labels.push(Label {
        target: LabelTarget::Local {
            range: range(type_start, type_start + 4),
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

const WIDGET_DECLARATION: &str =
    "<?php\nnamespace App;\n\nclass Widget\n{\n    public function save(): void\n    {\n    }\n}\n";

/// A `LabelTarget::Symbolic` label whose resolver returns
/// `ResolvedLabel::Concrete` in the diagnostic's own file: it must join
/// the primary snippet as a secondary underline, exactly like a `Local`
/// label, rather than becoming its own snippet element.
#[test]
fn a_symbolic_label_resolving_to_the_same_file_becomes_a_local_underline() {
    use celerrate_diagnostics::{Label, LabelTarget};

    let file = FileId::new(0);
    let member_start = CALLER.find("svae").unwrap() as u32;
    let member = range(member_start, member_start + 4);
    let mut diagnostic = Diagnostic::spanned(
        find_identifier("CEL0030").unwrap(),
        Severity::Error,
        file,
        member,
        "unknown method `svae` on `App\\User`".to_owned(),
    );
    diagnostic.labels.push(Label {
        target: LabelTarget::Symbolic {
            symbol: "App\\User".to_owned(),
        },
        message: "the receiver is typed `App\\User` here".to_owned(),
    });
    // Resolves inside CALLER itself: `User` in the signature.
    let type_start = CALLER.find("User $user").unwrap() as u32;
    let resolver = ScriptedResolver(vec![(
        "App\\User",
        ResolvedLabel::Concrete {
            file,
            range: range(type_start, type_start + 4),
        },
    )]);

    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![(file, "src/Caller.php", CALLER)]),
        &resolver,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("symbolic_label_same_file", report.blocks.join("\n\n"));
}

/// A `LabelTarget::Symbolic` label whose resolver returns
/// `ResolvedLabel::Concrete` in another file that the `SourceAccess`
/// fixture can excerpt: it must become its own foreign snippet element,
/// with that file's own display path and an annotated line.
#[test]
fn a_symbolic_label_resolving_to_another_file_becomes_a_foreign_snippet() {
    use celerrate_diagnostics::{Label, LabelTarget};

    let caller_file = FileId::new(0);
    let widget_file = FileId::new(1);
    let member_start = CALLER.find("svae").unwrap() as u32;
    let member = range(member_start, member_start + 4);
    let mut diagnostic = Diagnostic::spanned(
        find_identifier("CEL0030").unwrap(),
        Severity::Error,
        caller_file,
        member,
        "unknown method `svae` on `App\\User`".to_owned(),
    );
    diagnostic.labels.push(Label {
        target: LabelTarget::Symbolic {
            symbol: "App\\User::save".to_owned(),
        },
        message: "`save` is declared here".to_owned(),
    });
    let declaration_start = WIDGET_DECLARATION.find("save").unwrap() as u32;
    let resolver = ScriptedResolver(vec![(
        "App\\User::save",
        ResolvedLabel::Concrete {
            file: widget_file,
            range: range(declaration_start, declaration_start + 4),
        },
    )]);

    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![
            (caller_file, "src/Caller.php", CALLER),
            (widget_file, "src/Widget.php", WIDGET_DECLARATION),
        ]),
        &resolver,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("symbolic_label_foreign_file", report.blocks.join("\n\n"));
}

/// A `LabelTarget::Symbolic` label whose resolver returns
/// `ResolvedLabel::Degraded` directly (a stub with no source at all)
/// degrades to a `note:` line naming the declaration.
#[test]
fn a_degraded_symbolic_label_becomes_a_note() {
    use celerrate_diagnostics::{Label, LabelTarget};

    let file = FileId::new(0);
    let member_start = CALLER.find("svae").unwrap() as u32;
    let member = range(member_start, member_start + 4);
    let mut diagnostic = Diagnostic::spanned(
        find_identifier("CEL0030").unwrap(),
        Severity::Error,
        file,
        member,
        "unknown method `svae` on `App\\User`".to_owned(),
    );
    diagnostic.labels.push(Label {
        target: LabelTarget::Symbolic {
            symbol: "App\\User::save".to_owned(),
        },
        message: "declared in a stub with no source".to_owned(),
    });
    let resolver = ScriptedResolver(vec![("App\\User::save", ResolvedLabel::Degraded)]);

    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![(file, "src/Caller.php", CALLER)]),
        &resolver,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("symbolic_label_degraded", report.blocks.join("\n\n"));
}

/// A `LabelTarget::Symbolic` label whose resolver returns
/// `ResolvedLabel::Concrete` in a file the `SourceAccess` fixture does
/// not carry at all (so both `display_path` and `text` return `None`):
/// `plan_labels` falls back to the same degraded note form as an
/// explicit `ResolvedLabel::Degraded`.
#[test]
fn a_symbolic_label_resolving_to_an_uncarried_file_falls_back_to_a_note() {
    use celerrate_diagnostics::{Label, LabelTarget};

    let file = FileId::new(0);
    let missing_file = FileId::new(99);
    let member_start = CALLER.find("svae").unwrap() as u32;
    let member = range(member_start, member_start + 4);
    let mut diagnostic = Diagnostic::spanned(
        find_identifier("CEL0030").unwrap(),
        Severity::Error,
        file,
        member,
        "unknown method `svae` on `App\\User`".to_owned(),
    );
    diagnostic.labels.push(Label {
        target: LabelTarget::Symbolic {
            symbol: "App\\User::save".to_owned(),
        },
        message: "`save` is declared here".to_owned(),
    });
    let resolver = ScriptedResolver(vec![(
        "App\\User::save",
        ResolvedLabel::Concrete {
            file: missing_file,
            range: range(0, 4),
        },
    )]);

    let report = render_report(
        &[diagnostic],
        &FixtureSources(vec![(file, "src/Caller.php", CALLER)]),
        &resolver,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!(
        "symbolic_label_concrete_without_source",
        report.blocks.join("\n\n")
    );
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
