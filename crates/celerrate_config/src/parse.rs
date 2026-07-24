//! The structural walk: file text to `Configuration` plus structural
//! diagnostics (syntax, unknown keys, invalid values). Semantic checks
//! against the registries live in `validate`, not here.

use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::{FileId, TextRange, TextSize};

use crate::identifiers::{
    INVALID_CONFIGURATION, INVALID_CONFIGURATION_VALUE, UNKNOWN_CONFIGURATION_KEY,
};
use crate::model::{Configuration, Spanned};

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

/// The span of `key` in `table`, with the whole-table fallback that
/// keeps every diagnostic anchored even if `toml_edit` yields no span.
fn key_range(table: &toml_edit::Table, key: &str, fallback: TextRange) -> TextRange {
    table
        .key(key)
        .and_then(toml_edit::Key::span)
        .map_or(fallback, text_range)
}

/// The span of a value item, falling back to its key's span.
fn item_range(
    table: &toml_edit::Table,
    key: &str,
    item: &toml_edit::Item,
    fallback: TextRange,
) -> TextRange {
    item.span()
        .map_or_else(|| key_range(table, key, fallback), text_range)
}

fn unknown_key(file: FileId, range: TextRange, path: &str) -> Diagnostic {
    Diagnostic::spanned(
        UNKNOWN_CONFIGURATION_KEY,
        Severity::Error,
        file,
        range,
        format!("unknown configuration key `{path}`"),
    )
}

fn invalid_value(file: FileId, range: TextRange, key: &str, expectation: &str) -> Diagnostic {
    Diagnostic::spanned(
        INVALID_CONFIGURATION_VALUE,
        Severity::Error,
        file,
        range,
        format!("invalid value for `{key}`: {expectation}"),
    )
}

/// `"8.2"` as a `(major, minor)` point; `None` for every other shape
/// (ranges, carets, prose), which CEL0045 reports.
fn version_point(text: &str) -> Option<(u8, u8)> {
    let (major, minor) = text.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The root walk grows in the next tasks: the `[rules]`, `[severity]`
/// and `[plugins]` arms are Task 4, so those root keys fall through to
/// "unknown configuration key" until then.
fn walk_root(
    file: FileId,
    table: &toml_edit::Table,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // The whole-file anchor of last resort: used only when `toml_edit`
    // gives no span for a key or item, which does not happen on the
    // well-formed spans `Document::parse` produces but is guarded
    // against regardless. Identical to `fallback_range` on non-empty
    // text; a non-empty root table implies non-empty text.
    let unspanned_anchor = TextRange::new(TextSize::from(0), TextSize::from(1));
    for (key, item) in table.iter() {
        let range = key_range(table, key, unspanned_anchor);
        match key {
            "project" => match item.as_table() {
                Some(project) => {
                    walk_project(file, project, configuration, diagnostics, unspanned_anchor);
                }
                None => diagnostics.push(invalid_value(
                    file,
                    item_range(table, key, item, unspanned_anchor),
                    key,
                    "expected a table",
                )),
            },
            _ => diagnostics.push(unknown_key(file, range, key)),
        }
    }
}

fn walk_project(
    file: FileId,
    project: &toml_edit::Table,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
    fallback: TextRange,
) {
    for (key, item) in project.iter() {
        let value_range = item_range(project, key, item, fallback);
        match key {
            "php" => match item.as_str().and_then(version_point) {
                Some(point) => {
                    configuration.php = Some(Spanned {
                        value: point,
                        range: value_range,
                    });
                }
                None => diagnostics.push(invalid_value(
                    file,
                    value_range,
                    key,
                    "expected a version point like \"8.2\"",
                )),
            },
            "include" | "exclude" => {
                let entries = path_array(file, key, item, value_range, diagnostics);
                if key == "include" {
                    configuration.include = entries;
                } else {
                    configuration.exclude = entries;
                }
            }
            _ => diagnostics.push(unknown_key(
                file,
                key_range(project, key, fallback),
                &format!("project.{key}"),
            )),
        }
    }
}

/// An `include`/`exclude` array: non-empty relative path strings. A
/// malformed entry is reported and skipped; the well-formed ones stay.
fn path_array(
    file: FileId,
    key: &str,
    item: &toml_edit::Item,
    value_range: TextRange,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Spanned<String>> {
    let Some(array) = item.as_array() else {
        diagnostics.push(invalid_value(
            file,
            value_range,
            key,
            "expected an array of relative paths",
        ));
        return Vec::new();
    };
    let mut entries = Vec::new();
    for value in array.iter() {
        let entry_range = value.span().map_or(value_range, text_range);
        match value.as_str() {
            Some(path) if !path.is_empty() && !std::path::Path::new(path).is_absolute() => {
                entries.push(Spanned {
                    value: path.to_owned(),
                    range: entry_range,
                });
            }
            _ => diagnostics.push(invalid_value(
                file,
                entry_range,
                key,
                "expected a non-empty relative path",
            )),
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use celerrate_source::FileId;

    use crate::identifiers::{
        INVALID_CONFIGURATION, INVALID_CONFIGURATION_VALUE, UNKNOWN_CONFIGURATION_KEY,
    };
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

    fn single(
        diagnostics: &[celerrate_diagnostics::Diagnostic],
    ) -> &celerrate_diagnostics::Diagnostic {
        assert_eq!(diagnostics.len(), 1, "expected exactly one diagnostic");
        diagnostics.first().unwrap()
    }

    #[test]
    fn the_project_table_parses_php_include_and_exclude() {
        let text = "[project]\nphp = \"8.2\"\ninclude = [\"src\", \"tests\"]\nexclude = [\"src/Generated\"]\n";
        let (configuration, diagnostics) = parse(file(), text);
        assert!(diagnostics.is_empty());
        assert_eq!(configuration.php.as_ref().unwrap().value, (8, 2));
        let include: Vec<&str> = configuration
            .include
            .iter()
            .map(|entry| entry.value.as_str())
            .collect();
        assert_eq!(include, ["src", "tests"]);
        let exclude: Vec<&str> = configuration
            .exclude
            .iter()
            .map(|entry| entry.value.as_str())
            .collect();
        assert_eq!(exclude, ["src/Generated"]);
    }

    #[test]
    fn an_unknown_root_key_reports_cel0044() {
        let (_, diagnostics) = parse(file(), "reals = 1\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNKNOWN_CONFIGURATION_KEY);
        assert_eq!(diagnostic.message, "unknown configuration key `reals`");
    }

    #[test]
    fn an_unknown_project_key_reports_cel0044() {
        let (_, diagnostics) = parse(file(), "[project]\nincludes = [\"src\"]\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNKNOWN_CONFIGURATION_KEY);
        assert_eq!(
            diagnostic.message,
            "unknown configuration key `project.includes`"
        );
    }

    #[test]
    fn a_php_constraint_that_is_not_a_version_point_reports_cel0045() {
        let (configuration, diagnostics) = parse(file(), "[project]\nphp = \"^8.1\"\n");
        assert!(configuration.php.is_none());
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `php`: expected a version point like \"8.2\"",
        );
    }

    #[test]
    fn a_non_array_include_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "[project]\ninclude = \"src\"\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `include`: expected an array of relative paths",
        );
    }

    #[test]
    fn an_absolute_or_empty_include_entry_reports_cel0045() {
        let (configuration, diagnostics) = parse(file(), "[project]\ninclude = [\"/etc\", \"\"]\n");
        assert!(configuration.include.is_empty());
        assert_eq!(diagnostics.len(), 2);
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
            assert_eq!(
                diagnostic.message,
                "invalid value for `include`: expected a non-empty relative path",
            );
        }
    }
}
