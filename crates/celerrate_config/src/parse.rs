//! The structural walk: file text to `Configuration` plus structural
//! diagnostics (syntax, unknown keys, invalid values). Semantic checks
//! against the registries live in `validate`, not here.

use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_source::{FileId, TextRange, TextSize};

use crate::identifiers::{
    INVALID_CONFIGURATION, INVALID_CONFIGURATION_VALUE, UNKNOWN_CONFIGURATION_KEY,
    UNSUPPORTED_RULE_OPTION,
};
use crate::model::{Configuration, RuleEntry, SeverityEntry, Spanned};

/// A byte span from `toml_edit` as a `TextRange`. Configuration files
/// are far below `u32` size; a hypothetical overflow saturates rather
/// than panics.
fn text_range(span: core::ops::Range<usize>) -> TextRange {
    let start = u32::try_from(span.start).unwrap_or(u32::MAX);
    let end = u32::try_from(span.end).unwrap_or(u32::MAX);
    TextRange::new(TextSize::from(start), TextSize::from(end.max(start)))
}

/// The anchor of last resort, for a finding `toml_edit` gives no span
/// for: a zero-width range at the start of the file.
///
/// The width is deliberately zero rather than one byte. These offsets
/// index the file text when the finding is rendered, and a one-byte
/// range on a file whose first character is multi-byte would land
/// inside that character instead of on a boundary. A zero-width range
/// at offset 0 cannot split anything, and it is the honest width for an
/// anchor that points at no text in particular.
fn unspanned_anchor() -> TextRange {
    TextRange::new(TextSize::from(0), TextSize::from(0))
}

/// Parses `celerrate.toml` text. Never fails: what does not parse is a
/// diagnostic, and the configuration degrades to its default.
pub fn parse(file: FileId, text: &str) -> (Configuration, Vec<Diagnostic>) {
    let document = match toml_edit::Document::parse(text) {
        Ok(document) => document,
        Err(error) => {
            let range = error.span().map_or_else(unspanned_anchor, text_range);
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
    // Deliberately unsorted: every caller concatenates this with the
    // semantic diagnostics from `validate` and sorts the whole, so
    // sorting here would be work whose result is immediately discarded.
    (configuration, diagnostics)
}

/// The span of `key` in `table`, with the whole-table fallback that
/// keeps every diagnostic anchored even if `toml_edit` yields no span.
fn key_range(table: &dyn toml_edit::TableLike, key: &str, fallback: TextRange) -> TextRange {
    table
        .key(key)
        .and_then(toml_edit::Key::span)
        .map_or(fallback, text_range)
}

/// The span of a value item, falling back to its key's span.
fn item_range(
    table: &dyn toml_edit::TableLike,
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

/// The table `path` holds, in either spelling: `[path]` on its own line
/// or `path = { ... }` inline. `as_table_like` is what makes the two
/// equivalent; `as_table` matches only the first and would reject TOML
/// the user is entitled to write. `None` means the value is genuinely
/// not a table, which this reports as CEL0045 before returning, so the
/// cast and its diagnostic live in exactly one place.
fn table_or_report<'item>(
    file: FileId,
    item: &'item toml_edit::Item,
    range: TextRange,
    path: &str,
    expectation: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'item dyn toml_edit::TableLike> {
    let nested = item.as_table_like();
    if nested.is_none() {
        diagnostics.push(invalid_value(file, range, path, expectation));
    }
    nested
}

/// The structural walk of the root table: `[project]`, the
/// `[rules.<name>]` tables, `[severity]`, and `[plugins]`. Anything else
/// is an unknown root key.
fn walk_root(
    file: FileId,
    table: &dyn toml_edit::TableLike,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Used only when `toml_edit` gives no span for a key or item, which
    // does not happen on the well-formed spans `Document::parse`
    // produces but is guarded against regardless.
    let anchor = unspanned_anchor();
    for (key, item) in table.iter() {
        let range = item_range(table, key, item, anchor);
        match key {
            "project" => {
                if let Some(project) =
                    table_or_report(file, item, range, key, "expected a table", diagnostics)
                {
                    walk_project(file, project, configuration, diagnostics, anchor);
                }
            }
            "rules" => {
                let expectation = "expected a table of [rules.<name>] tables";
                if let Some(rules) =
                    table_or_report(file, item, range, key, expectation, diagnostics)
                {
                    walk_rules(file, rules, configuration, diagnostics, anchor);
                }
            }
            "severity" => {
                if let Some(severity) =
                    table_or_report(file, item, range, key, "expected a table", diagnostics)
                {
                    walk_severity(file, severity, configuration, diagnostics, anchor);
                }
            }
            "plugins" => {
                if let Some(plugins) =
                    table_or_report(file, item, range, key, "expected a table", diagnostics)
                {
                    walk_plugins(file, plugins, diagnostics, anchor);
                }
            }
            _ => diagnostics.push(unknown_key(file, key_range(table, key, anchor), key)),
        }
    }
}

/// The `[plugins]` table: no plugin can be loaded yet, so every key it
/// holds is an unknown configuration key. The table itself is accepted
/// so that the diagnostic names the plugin, not the section.
fn walk_plugins(
    file: FileId,
    plugins: &dyn toml_edit::TableLike,
    diagnostics: &mut Vec<Diagnostic>,
    fallback: TextRange,
) {
    for (key, _) in plugins.iter() {
        diagnostics.push(unknown_key(
            file,
            key_range(plugins, key, fallback),
            &format!("plugins.{key}"),
        ));
    }
}

/// The `[rules.<name>]` tables: each is a table with at most one key,
/// `enabled`. Any other key has nowhere to go, since no rule takes
/// options yet.
fn walk_rules(
    file: FileId,
    rules: &dyn toml_edit::TableLike,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
    fallback: TextRange,
) {
    for (name, item) in rules.iter() {
        let name_range = key_range(rules, name, fallback);
        let Some(rule) = table_or_report(
            file,
            item,
            item_range(rules, name, item, fallback),
            &format!("rules.{name}"),
            &format!("expected a table like [rules.{name}]"),
            diagnostics,
        ) else {
            continue;
        };
        let mut enabled = None;
        for (key, value) in rule.iter() {
            let value_range = item_range(rule, key, value, fallback);
            if key == "enabled" {
                match value.as_bool() {
                    Some(flag) => {
                        enabled = Some(Spanned {
                            value: flag,
                            range: value_range,
                        });
                    }
                    None => diagnostics.push(invalid_value(
                        file,
                        value_range,
                        "enabled",
                        "expected a boolean",
                    )),
                }
            } else {
                diagnostics.push(Diagnostic::spanned(
                    UNSUPPORTED_RULE_OPTION,
                    Severity::Error,
                    file,
                    key_range(rule, key, fallback),
                    format!("rule `{name}` has no configurable options; `{key}` is not recognized"),
                ));
            }
        }
        configuration.rules.push(RuleEntry {
            name: Spanned {
                value: name.to_owned(),
                range: name_range,
            },
            enabled,
        });
    }
}

/// The `[severity]` table: identifier keys mapped to `"error"` or
/// `"warning"`. Whether the identifier is known is a semantic check
/// left to `validate`.
fn walk_severity(
    file: FileId,
    severity: &dyn toml_edit::TableLike,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
    fallback: TextRange,
) {
    for (identifier, item) in severity.iter() {
        let identifier_range = key_range(severity, identifier, fallback);
        let value_range = item_range(severity, identifier, item, fallback);
        let parsed = match item.as_str() {
            Some("error") => Some(Severity::Error),
            Some("warning") => Some(Severity::Warning),
            _ => None,
        };
        let severity = match parsed {
            Some(value) => Some(Spanned {
                value,
                range: value_range,
            }),
            None => {
                diagnostics.push(invalid_value(
                    file,
                    value_range,
                    &format!("severity.{identifier}"),
                    "expected \"error\" or \"warning\"",
                ));
                None
            }
        };
        configuration.severity.push(SeverityEntry {
            identifier: Spanned {
                value: identifier.to_owned(),
                range: identifier_range,
            },
            severity,
        });
    }
}

fn walk_project(
    file: FileId,
    project: &dyn toml_edit::TableLike,
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

/// Whether `path` looks like an absolute path, judged from the string's
/// own syntax rather than the host platform's rules.
///
/// This deliberately does not delegate to `std::path::Path::is_absolute`:
/// that method's semantics are platform-dependent, and `celerrate.toml`
/// is a file users commit to a repository that may be analysed on any
/// of Linux, macOS, or Windows. On Unix, `Path::new("/etc").is_absolute()`
/// is `true` but `Path::new("C:/Windows").is_absolute()` is `false`; on
/// Windows it is the other way around (`is_absolute` there requires a
/// drive or UNC prefix, so a bare `"/etc"` does not count). Delegating
/// would make the same committed configuration produce different
/// diagnostics depending on which OS runs the tool. This predicate
/// instead treats a leading `/` or `\`, or a Windows drive prefix
/// (a single ASCII letter, `:`, then nothing or a slash), as absolute on
/// every platform. Do not "simplify" this back to `Path::is_absolute`.
fn looks_absolute(path: &str) -> bool {
    let mut chars = path.chars();
    match chars.next() {
        Some('/' | '\\') => true,
        Some(letter) if letter.is_ascii_alphabetic() => match chars.next() {
            Some(':') => matches!(chars.next(), None | Some('/' | '\\')),
            _ => false,
        },
        _ => false,
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
            Some(path) if !path.is_empty() && !looks_absolute(path) => {
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
        UNSUPPORTED_RULE_OPTION,
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

    #[test]
    fn a_windows_drive_qualified_include_entry_reports_cel0045() {
        let (configuration, diagnostics) = parse(file(), "[project]\ninclude = [\"C:/Windows\"]\n");
        assert!(configuration.include.is_empty());
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `include`: expected a non-empty relative path",
        );
    }

    #[test]
    fn a_backslash_rooted_include_entry_reports_cel0045() {
        let (configuration, diagnostics) =
            parse(file(), "[project]\ninclude = [\"\\\\Windows\"]\n");
        assert!(configuration.include.is_empty());
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `include`: expected a non-empty relative path",
        );
    }

    #[test]
    fn an_ordinary_relative_include_entry_is_still_accepted() {
        let (configuration, diagnostics) =
            parse(file(), "[project]\ninclude = [\"src/Generated\"]\n");
        assert!(diagnostics.is_empty());
        let include: Vec<&str> = configuration
            .include
            .iter()
            .map(|entry| entry.value.as_str())
            .collect();
        assert_eq!(include, ["src/Generated"]);
    }

    #[test]
    fn a_mixed_include_array_skips_only_the_malformed_entry() {
        let (configuration, diagnostics) =
            parse(file(), "[project]\ninclude = [\"/etc\", \"src\"]\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `include`: expected a non-empty relative path",
        );
        let include: Vec<&str> = configuration
            .include
            .iter()
            .map(|entry| entry.value.as_str())
            .collect();
        assert_eq!(include, ["src"]);
    }

    #[test]
    fn a_rule_table_parses_its_enabled_flag() {
        let text = "[rules.null-dereference]\nenabled = false\n\n[rules.some-nursery-rule]\nenabled = true\n";
        let (configuration, diagnostics) = parse(file(), text);
        assert!(diagnostics.is_empty());
        let rules: Vec<(&str, Option<bool>)> = configuration
            .rules
            .iter()
            .map(|rule| {
                (
                    rule.name.value.as_str(),
                    rule.enabled.as_ref().map(|flag| flag.value),
                )
            })
            .collect();
        assert_eq!(
            rules,
            [
                ("null-dereference", Some(false)),
                ("some-nursery-rule", Some(true))
            ],
        );
    }

    #[test]
    fn an_empty_rule_table_is_a_valid_no_op() {
        let (configuration, diagnostics) = parse(file(), "[rules.null-dereference]\n");
        assert!(diagnostics.is_empty());
        assert_eq!(configuration.rules.first().unwrap().enabled, None);
    }

    #[test]
    fn a_rule_option_other_than_enabled_reports_cel0047() {
        let (_, diagnostics) = parse(file(), "[rules.null-dereference]\nmax = 3\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNSUPPORTED_RULE_OPTION);
        assert_eq!(
            diagnostic.message,
            "rule `null-dereference` has no configurable options; `max` is not recognized",
        );
    }

    #[test]
    fn a_non_boolean_enabled_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "[rules.null-dereference]\nenabled = \"yes\"\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `enabled`: expected a boolean"
        );
    }

    #[test]
    fn a_rules_entry_that_is_not_a_table_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "[rules]\ndisable = [\"null-dereference\"]\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `rules.disable`: expected a table like [rules.disable]",
        );
    }

    #[test]
    fn severity_entries_parse_and_reject_other_words() {
        let text = "[severity]\n\"CEL0034\" = \"warning\"\n\"CEL0035\" = \"info\"\n";
        let (configuration, diagnostics) = parse(file(), text);
        assert_eq!(configuration.severity.len(), 2);
        let valid_entry = &configuration.severity[0];
        assert_eq!(valid_entry.identifier.value, "CEL0034");
        assert_eq!(
            valid_entry.severity.as_ref().map(|spanned| spanned.value),
            Some(celerrate_diagnostics::Severity::Warning)
        );
        let invalid_entry = &configuration.severity[1];
        assert_eq!(invalid_entry.identifier.value, "CEL0035");
        assert!(invalid_entry.severity.is_none());
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `severity.CEL0035`: expected \"error\" or \"warning\"",
        );
    }

    /// The same `Configuration` with every span collapsed to the same
    /// range. Two spellings of the same configuration hold the same
    /// values at different byte offsets; this compares everything but
    /// the offsets, which is what "the same configuration" means here.
    fn without_spans(configuration: crate::Configuration) -> crate::Configuration {
        use celerrate_source::{TextRange, TextSize};

        let zero = TextRange::new(TextSize::from(0), TextSize::from(0));
        fn flatten<T>(
            spanned: crate::model::Spanned<T>,
            zero: celerrate_source::TextRange,
        ) -> crate::model::Spanned<T> {
            crate::model::Spanned {
                value: spanned.value,
                range: zero,
            }
        }
        crate::Configuration {
            php: configuration.php.map(|php| flatten(php, zero)),
            include: configuration
                .include
                .into_iter()
                .map(|entry| flatten(entry, zero))
                .collect(),
            exclude: configuration
                .exclude
                .into_iter()
                .map(|entry| flatten(entry, zero))
                .collect(),
            rules: configuration
                .rules
                .into_iter()
                .map(|rule| crate::model::RuleEntry {
                    name: flatten(rule.name, zero),
                    enabled: rule.enabled.map(|enabled| flatten(enabled, zero)),
                })
                .collect(),
            severity: configuration
                .severity
                .into_iter()
                .map(|entry| crate::model::SeverityEntry {
                    identifier: flatten(entry.identifier, zero),
                    severity: entry.severity.map(|severity| flatten(severity, zero)),
                })
                .collect(),
        }
    }

    /// Both spellings parse to the same configuration and say nothing.
    fn assert_same_configuration(header: &str, inline: &str) {
        let (from_header, header_diagnostics) = parse(file(), header);
        let (from_inline, inline_diagnostics) = parse(file(), inline);
        assert!(
            header_diagnostics.is_empty(),
            "the header spelling is silent: {header_diagnostics:?}",
        );
        assert!(
            inline_diagnostics.is_empty(),
            "the inline spelling is silent: {inline_diagnostics:?}",
        );
        assert_eq!(
            without_spans(from_inline),
            without_spans(from_header),
            "the inline spelling parses to the same configuration",
        );
    }

    #[test]
    fn an_inline_project_table_parses_like_the_header_spelling() {
        assert_same_configuration(
            "[project]\nphp = \"8.2\"\ninclude = [\"src\"]\n",
            "project = { php = \"8.2\", include = [\"src\"] }\n",
        );
    }

    #[test]
    fn an_inline_rule_entry_parses_like_the_header_spelling() {
        let text = "[rules]\nnull-dereference = { enabled = false }\n";
        assert_same_configuration("[rules.null-dereference]\nenabled = false\n", text);
        let (configuration, diagnostics) = parse(file(), text);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(configuration.rules.len(), 1);
        let rule = configuration.rules.first().unwrap();
        assert_eq!(rule.name.value, "null-dereference");
        assert_eq!(rule.enabled.as_ref().map(|flag| flag.value), Some(false));
    }

    #[test]
    fn a_fully_inline_rules_table_parses_like_the_header_spelling() {
        assert_same_configuration(
            "[rules.null-dereference]\nenabled = false\n\n[rules.unknown-members]\nenabled = false\n",
            "rules = { null-dereference = { enabled = false }, unknown-members = { enabled = false } }\n",
        );
    }

    #[test]
    fn an_inline_severity_table_parses_like_the_header_spelling() {
        assert_same_configuration(
            "[severity]\n\"CEL0034\" = \"warning\"\n",
            "severity = { \"CEL0034\" = \"warning\" }\n",
        );
    }

    /// `Key::span` must survive inside an inline table: the CEL0047
    /// anchor comes from `TableLike::key`, and a silent loss of the span
    /// would degrade every inline diagnostic to the whole-file anchor.
    #[test]
    fn a_diagnostic_from_inside_an_inline_table_keeps_its_span() {
        let text = "[rules]\nnull-dereference = { max = 3 }\n";
        let (_, diagnostics) = parse(file(), text);
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNSUPPORTED_RULE_OPTION);
        let (_, range) = diagnostic.span().expect("inline diagnostics are anchored");
        let start = usize::from(range.start());
        let end = usize::from(range.end());
        assert_eq!(
            &text[start..end],
            "max",
            "the span points at the key inside the inline table",
        );
    }

    /// The value span inside an inline table, which comes from
    /// `Item::span` rather than `Key::span`.
    #[test]
    fn a_value_diagnostic_from_inside_an_inline_table_keeps_its_span() {
        let text = "severity = { \"CEL0035\" = \"info\" }\n";
        let (_, diagnostics) = parse(file(), text);
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        let (_, range) = diagnostic.span().expect("inline diagnostics are anchored");
        let start = usize::from(range.start());
        let end = usize::from(range.end());
        assert_eq!(&text[start..end], "\"info\"");
    }

    #[test]
    fn a_project_value_that_is_not_a_table_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "project = 1\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `project`: expected a table",
        );
    }

    #[test]
    fn a_rules_value_that_is_not_a_table_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "rules = \"oops\"\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `rules`: expected a table of [rules.<name>] tables",
        );
    }

    #[test]
    fn a_severity_value_that_is_not_a_table_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "severity = 5\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `severity`: expected a table",
        );
    }

    #[test]
    fn a_plugins_value_that_is_not_a_table_reports_cel0045() {
        let (_, diagnostics) = parse(file(), "plugins = [1]\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `plugins`: expected a table",
        );
    }

    #[test]
    fn a_plugins_key_reports_cel0044() {
        let (_, diagnostics) = parse(file(), "[plugins]\nphpdoc-bridge = true\n");
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, UNKNOWN_CONFIGURATION_KEY);
        assert_eq!(
            diagnostic.message,
            "unknown configuration key `plugins.phpdoc-bridge`"
        );
    }
}
