//! The rustc-style renderer: a pure function from enriched
//! diagnostics plus sources to text (design section 9).
//!
//! Everything here runs OUTSIDE salsa queries, at presentation time.
//! `adapter.rs` is the only module that references `annotate-snippets`;
//! `resolve.rs` resolves symbolic labels against the database at render
//! time. Color, TTY detection, and terminal size are the CLI's
//! business: this module receives a [`ColorMode`] and never reads the
//! environment.

use celerrate_diagnostics::{Anchor, Diagnostic};
use celerrate_source::{FileId, LineIndex};

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
}
