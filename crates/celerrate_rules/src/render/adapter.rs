//! The one module that maps the diagnostic anatomy onto
//! `annotate-snippets` input types. Keeping the mapping here keeps the
//! library replaceable: nothing else references it (design section 9).

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::{FileId, TextRange};

use super::{ColorMode, FaultInjection, SourceAccess, SymbolResolver};

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
    // `sources` and `resolver` feed label planning in Task 3 (labels
    // resolved against symbolic declarations); `fault` feeds the
    // fault-injection seam Task 4 adds around this call.
    let _ = (sources, resolver, fault, file);
    Some(build(diagnostic, path, text, color))
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

fn build(diagnostic: &Diagnostic, path: &str, text: &str, color: ColorMode) -> String {
    let range = match diagnostic.anchor {
        celerrate_diagnostics::Anchor::Span { range, .. } => range,
        celerrate_diagnostics::Anchor::Project => TextRange::empty(0.into()),
    };
    let snippet = Snippet::source(text)
        .path(path)
        .line_start(1)
        .fold(true)
        .annotation(AnnotationKind::Primary.span(to_usize_range(range)));
    let group = level_of(diagnostic.severity)
        .primary_title(diagnostic.message.as_str())
        .id(diagnostic.id.as_str())
        .element(snippet);
    let renderer = match color {
        ColorMode::Plain => Renderer::plain(),
        ColorMode::Styled => Renderer::styled(),
    };
    renderer.render(&[group]).to_string()
}
