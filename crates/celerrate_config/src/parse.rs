//! The structural walk: file text to `Configuration` plus structural
//! diagnostics (syntax, unknown keys, invalid values). Semantic checks
//! against the registries live in `validate`, not here.

use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::{FileId, TextRange, TextSize};

use crate::identifiers::INVALID_CONFIGURATION;
use crate::model::Configuration;

/// A byte span from `toml_edit` as a `TextRange`. Configuration files
/// are far below `u32` size; a hypothetical overflow saturates rather
/// than panics.
fn text_range(span: core::ops::Range<usize>) -> TextRange {
    let start = u32::try_from(span.start).unwrap_or(u32::MAX);
    let end = u32::try_from(span.end).unwrap_or(u32::MAX);
    TextRange::new(TextSize::from(start), TextSize::from(end.max(start)))
}

/// The whole-file fallback anchor for findings the parser gives no
/// span for: the first byte, or an empty range on an empty file.
fn fallback_range(text: &str) -> TextRange {
    let end = u32::from(!text.is_empty());
    TextRange::new(TextSize::from(0), TextSize::from(end))
}

/// Parses `celerrate.toml` text. Never fails: what does not parse is a
/// diagnostic, and the configuration degrades to its default.
pub fn parse(file: FileId, text: &str) -> (Configuration, Vec<Diagnostic>) {
    let document = match toml_edit::Document::parse(text) {
        Ok(document) => document,
        Err(error) => {
            let range = error
                .span()
                .map_or_else(|| fallback_range(text), text_range);
            let diagnostic = Diagnostic::spanned(
                INVALID_CONFIGURATION,
                Severity::Error,
                file,
                range,
                format!("invalid TOML: {}", error.message()),
            );
            return (Configuration::default(), vec![diagnostic]);
        }
    };
    let mut configuration = Configuration::default();
    let mut diagnostics = Vec::new();
    walk_root(
        file,
        document.as_table(),
        &mut configuration,
        &mut diagnostics,
    );
    diagnostics.sort();
    (configuration, diagnostics)
}

/// The root walk grows in the next tasks; for now every top-level key
/// is accepted silently so the syntax slice lands alone.
fn walk_root(
    _file: FileId,
    _table: &toml_edit::Table,
    _configuration: &mut Configuration,
    _diagnostics: &mut Vec<Diagnostic>,
) {
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use celerrate_source::FileId;

    use crate::identifiers::INVALID_CONFIGURATION;
    use crate::parse::parse;

    fn file() -> FileId {
        FileId::new(0)
    }

    #[test]
    fn an_empty_file_is_an_empty_configuration() {
        let (configuration, diagnostics) = parse(file(), "");
        assert_eq!(configuration, crate::Configuration::default());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_syntax_error_reports_cel0043_with_a_span() {
        let (configuration, diagnostics) = parse(file(), "[project\n");
        assert_eq!(configuration, crate::Configuration::default());
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.first().unwrap();
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION);
        assert!(
            diagnostic.span().is_some(),
            "syntax errors are span-anchored"
        );
        assert!(diagnostic.message.starts_with("invalid TOML:"));
    }
}
