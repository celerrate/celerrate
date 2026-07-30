//! The rustc-style renderer: a pure function from enriched
//! diagnostics plus sources to text.
//!
//! Everything here runs OUTSIDE salsa queries, at presentation time.
//! `adapter.rs` is the only module that references `annotate-snippets`;
//! `resolve.rs` resolves symbolic labels against the database at render
//! time. Color, TTY detection, and terminal size are the CLI's
//! business: this module receives a [`ColorMode`] and never reads the
//! environment.

use celerrate_diagnostics::{Anchor, Diagnostic, DiagnosticId};
use celerrate_source::{FileId, LineIndex, TextRange};

mod adapter;
pub mod resolve;

pub use adapter::degraded_note;

/// Read access to the sources a rendered report excerpts. The CLI
/// implements this over its session; tests implement it over fixtures.
pub trait SourceAccess {
    /// The project-relative display path of a file.
    fn display_path(&self, file: FileId) -> Option<String>;
    /// The decoded source text of a file.
    fn text(&self, file: FileId) -> Option<&str>;
}

/// The minimal one-line format: the fallback of every rich block, and
/// the preview format it replaces, byte for byte.
/// `path:line:column identifier message` (one-based), or the notice
/// line for a project-anchored finding.
pub fn render_minimal(diagnostic: &Diagnostic, sources: &dyn SourceAccess) -> String {
    match diagnostic.anchor {
        Anchor::Project => {
            format!("notice {}: {}", diagnostic.id.as_str(), diagnostic.message)
        }
        Anchor::Span { file, range } => {
            let path = sources
                .display_path(file)
                .unwrap_or_else(|| "<unknown>".to_owned());
            let (line, column) = match sources.text(file) {
                Some(text) => {
                    let position = LineIndex::new(text).line_column(range.start());
                    (position.line + 1, position.column + 1)
                }
                None => (1, 1),
            };
            format!(
                "{path}:{line}:{column} {} {}",
                diagnostic.id.as_str(),
                diagnostic.message,
            )
        }
    }
}

/// Whether the rendered text carries ANSI styling. Decided by the CLI
/// (TTY detection, `NO_COLOR`) outside queries; snapshots pin `Plain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Plain,
    Styled,
}

/// A symbolic label resolved at render time: a
/// concrete location when the declaration is VFS-backed and locatable,
/// or a degraded form rendered as a note naming the declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedLabel {
    Concrete { file: FileId, range: TextRange },
    Degraded,
}

/// Resolves a symbolic label's declaration path to a location. The
/// database-backed implementation lives in [`resolve`]; tests and
/// database-free callers use [`DegradeEverything`].
pub trait SymbolResolver {
    fn resolve(&self, symbol: &str) -> ResolvedLabel;
}

/// The resolver for contexts with no database at hand: every symbolic
/// label degrades to its note form.
pub struct DegradeEverything;

impl SymbolResolver for DegradeEverything {
    fn resolve(&self, _symbol: &str) -> ResolvedLabel {
        ResolvedLabel::Degraded
    }
}

/// Forces the rich path of matching diagnostics to fail, so the
/// fallback path is snapshot-tested rather than merely asserted
/// Always [`FaultInjection::None`] in production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultInjection {
    None,
    ForIdentifier(DiagnosticId),
}

/// One diagnostic whose rich rendering failed and fell back to the
/// minimal line. The CLI reports each as an internal error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderFailure {
    pub id: DiagnosticId,
    pub location: String,
}

/// The rendered report: one text block per diagnostic, in input
/// order, plus the rich-rendering failures the caller must surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedReport {
    pub blocks: Vec<String>,
    pub failures: Vec<RenderFailure>,
}

/// Renders every diagnostic to its block: the notice line for a
/// project-anchored finding, the rustc-style block for a span-anchored
/// one, the minimal line when rich rendering fails.
pub fn render_report(
    diagnostics: &[Diagnostic],
    sources: &dyn SourceAccess,
    resolver: &dyn SymbolResolver,
    color: ColorMode,
    fault: &FaultInjection,
) -> RenderedReport {
    let mut blocks = Vec::new();
    let mut failures = Vec::new();
    for diagnostic in diagnostics {
        match diagnostic.anchor {
            Anchor::Project => blocks.push(render_minimal(diagnostic, sources)),
            Anchor::Span { file, .. } => {
                let rich = sources.display_path(file).and_then(|path| {
                    let text = sources.text(file)?;
                    adapter::rich_block(
                        diagnostic, &path, text, file, sources, resolver, color, fault,
                    )
                });
                match rich {
                    Some(block) => blocks.push(block),
                    None => {
                        let line = render_minimal(diagnostic, sources);
                        failures.push(RenderFailure {
                            id: diagnostic.id,
                            location: line
                                .split_whitespace()
                                .next()
                                .unwrap_or("<unknown>")
                                .to_owned(),
                        });
                        blocks.push(line);
                    }
                }
            }
        }
    }
    RenderedReport { blocks, failures }
}

/// The report trailer that makes `celerrate explain` discoverable from
/// the primary output: one pointer per distinct
/// identifier reported, in identifier order.
pub fn explain_pointers(identifiers: impl IntoIterator<Item = DiagnosticId>) -> String {
    let mut seen: Vec<DiagnosticId> = identifiers.into_iter().collect();
    seen.sort();
    seen.dedup();
    seen.iter()
        .map(|id| {
            format!(
                "for more information, run `celerrate explain {}`\n",
                id.as_str()
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_diagnostics::{Diagnostic, Severity, find_identifier};
    use celerrate_source::{FileId, TextRange, TextSize};

    use super::{SourceAccess, render_minimal};

    pub(crate) struct FixtureSources(pub(crate) Vec<(FileId, &'static str, &'static str)>);

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

    #[test]
    fn a_span_diagnostic_renders_one_based_path_line_and_column() {
        let sources = FixtureSources(vec![(
            FileId::new(0),
            "src/Kernel.php",
            "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n",
        )]);
        let diagnostic = Diagnostic::spanned(
            find_identifier("CEL0018").unwrap(),
            Severity::Error,
            FileId::new(0),
            range(43, 50),
            "unknown class `Missing`".to_owned(),
        );
        assert_eq!(
            render_minimal(&diagnostic, &sources),
            "src/Kernel.php:4:22 CEL0018 unknown class `Missing`",
        );
    }

    #[test]
    fn a_project_diagnostic_renders_the_notice_line() {
        let sources = FixtureSources(vec![]);
        let diagnostic = Diagnostic::project(
            find_identifier("CEL0025").unwrap(),
            Severity::Warning,
            "no composer.json found; analyzing the whole project root".to_owned(),
        );
        assert_eq!(
            render_minimal(&diagnostic, &sources),
            "notice CEL0025: no composer.json found; analyzing the whole project root",
        );
    }

    #[test]
    fn a_missing_source_still_renders_a_line() {
        let sources = FixtureSources(vec![]);
        let diagnostic = Diagnostic::spanned(
            find_identifier("CEL0018").unwrap(),
            Severity::Error,
            FileId::new(7),
            range(0, 1),
            "unknown class `Missing`".to_owned(),
        );
        assert_eq!(
            render_minimal(&diagnostic, &sources),
            "<unknown>:1:1 CEL0018 unknown class `Missing`",
        );
    }

    #[test]
    fn explain_pointers_are_sorted_deduplicated_and_newline_terminated() {
        use super::explain_pointers;
        let identifiers = [
            find_identifier("CEL0030").unwrap(),
            find_identifier("CEL0018").unwrap(),
            find_identifier("CEL0030").unwrap(),
        ];
        assert_eq!(
            explain_pointers(identifiers),
            "for more information, run `celerrate explain CEL0018`\n\
             for more information, run `celerrate explain CEL0030`\n",
        );
    }

    #[test]
    fn no_identifiers_produce_no_pointer_text() {
        use super::explain_pointers;
        assert_eq!(explain_pointers([]), "");
    }
}
