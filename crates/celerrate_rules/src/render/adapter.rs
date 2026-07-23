//! The one module that maps the diagnostic anatomy onto
//! `annotate-snippets` input types. Keeping the mapping here keeps the
//! library replaceable: nothing else references it (design section 9).

use std::panic::{AssertUnwindSafe, catch_unwind};

use annotate_snippets::{AnnotationKind, Level, Patch, Renderer, Snippet};
use celerrate_diagnostics::{Anchor, Diagnostic, LabelTarget, Severity};
use celerrate_source::{FileId, TextRange};

use super::{ColorMode, FaultInjection, ResolvedLabel, SourceAccess, SymbolResolver};

/// Renders one span-anchored diagnostic as a rustc-style block.
/// `None` means the rich path failed; the caller falls back to the
/// minimal line and records the failure.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rich_block(
    diagnostic: &Diagnostic,
    path: &str,
    text: &str,
    file: FileId,
    sources: &dyn SourceAccess,
    resolver: &dyn SymbolResolver,
    color: ColorMode,
    fault: &FaultInjection,
) -> Option<String> {
    if let FaultInjection::ForIdentifier(id) = fault
        && *id == diagnostic.id
    {
        return None;
    }
    // `annotate-snippets` is not under the workspace zero-panic lints,
    // so one diagnostic's rendering panic must not take the report
    // down: it falls back to the minimal line (design section 9).
    catch_unwind(AssertUnwindSafe(|| {
        build(diagnostic, path, text, file, sources, resolver, color)
    }))
    .ok()
}

fn level_of(severity: Severity) -> Level<'static> {
    match severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
    }
}

fn to_usize_range(range: TextRange) -> core::ops::Range<usize> {
    u32::from(range.start()) as usize..u32::from(range.end()) as usize
}

/// Everything the block borrows must outlive the `render` call, so the
/// owned strings (foreign paths, degraded notes) are gathered first and
/// the `annotate-snippets` structures borrow from this plan.
struct BlockPlan<'s> {
    foreign: Vec<(String, &'s str, TextRange, String)>,
    degraded: Vec<String>,
}

fn plan_labels<'s>(
    diagnostic: &Diagnostic,
    file: FileId,
    sources: &'s dyn SourceAccess,
    resolver: &dyn SymbolResolver,
) -> (Vec<(TextRange, String)>, BlockPlan<'s>) {
    let mut local = Vec::new();
    let mut plan = BlockPlan {
        foreign: Vec::new(),
        degraded: Vec::new(),
    };
    for label in &diagnostic.labels {
        match &label.target {
            LabelTarget::Local { range } => local.push((*range, label.message.clone())),
            LabelTarget::Symbolic { symbol } => match resolver.resolve(symbol) {
                ResolvedLabel::Concrete { file: other, range } if other == file => {
                    local.push((range, label.message.clone()));
                }
                ResolvedLabel::Concrete { file: other, range } => {
                    match (sources.display_path(other), sources.text(other)) {
                        (Some(path), Some(text)) => {
                            plan.foreign
                                .push((path, text, range, label.message.clone()));
                        }
                        _ => plan.degraded.push(degraded_note(symbol, &label.message)),
                    }
                }
                ResolvedLabel::Degraded => {
                    plan.degraded.push(degraded_note(symbol, &label.message));
                }
            },
        }
    }
    (local, plan)
}

/// A symbolic label whose declaration has no excerptable source
/// degrades to a note naming the declaration (design section 3).
fn degraded_note(symbol: &str, message: &str) -> String {
    format!("`{symbol}`: {message}")
}

fn build(
    diagnostic: &Diagnostic,
    path: &str,
    text: &str,
    file: FileId,
    sources: &dyn SourceAccess,
    resolver: &dyn SymbolResolver,
    color: ColorMode,
) -> String {
    let range = match diagnostic.anchor {
        Anchor::Span { range, .. } => range,
        Anchor::Project => TextRange::empty(0.into()),
    };
    let (local_labels, plan) = plan_labels(diagnostic, file, sources, resolver);

    let mut snippet = Snippet::source(text)
        .path(path)
        .line_start(1)
        .fold(true)
        .annotation(AnnotationKind::Primary.span(to_usize_range(range)));
    for (label_range, message) in &local_labels {
        snippet = snippet.annotation(
            AnnotationKind::Context
                .span(to_usize_range(*label_range))
                .label(message.as_str()),
        );
    }

    let mut group = level_of(diagnostic.severity)
        .primary_title(diagnostic.message.as_str())
        .id(diagnostic.id.as_str())
        .element(snippet);

    for (foreign_path, foreign_text, foreign_range, message) in &plan.foreign {
        group = group.element(
            Snippet::source(*foreign_text)
                .path(foreign_path.as_str())
                .line_start(1)
                .fold(true)
                .annotation(
                    AnnotationKind::Context
                        .span(to_usize_range(*foreign_range))
                        .label(message.as_str()),
                ),
        );
    }
    for note in &plan.degraded {
        group = group.element(Level::NOTE.message(note.as_str()));
    }
    for note in &diagnostic.notes {
        group = group.element(Level::NOTE.message(note.as_str()));
    }
    for suggestion in &diagnostic.suggestions {
        group = group.element(Level::HELP.message(suggestion.message.as_str()));
        let same_file_edits: Vec<_> = suggestion
            .edits
            .iter()
            .filter(|edit| edit.file == file)
            .collect();
        if !same_file_edits.is_empty() {
            let mut patched = Snippet::source(text).path(path).line_start(1).fold(true);
            for edit in same_file_edits {
                patched = patched.patch(Patch::new(
                    to_usize_range(edit.range),
                    edit.replacement.as_str(),
                ));
            }
            group = group.element(patched);
        }
    }

    let renderer = match color {
        ColorMode::Plain => Renderer::plain(),
        ColorMode::Styled => Renderer::styled(),
    };
    renderer.render(&[group]).to_string()
}
