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

/// The structural walk of the root table: `[project]`, the
/// `[rules.<name>]` tables, `[severity]`, and `[plugins]`. Anything else
/// is an unknown root key.
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
            "rules" => match item.as_table() {
                Some(rules) => {
                    walk_rules(file, rules, configuration, diagnostics, unspanned_anchor);
                }
                None => diagnostics.push(invalid_value(
                    file,
                    item_range(table, key, item, unspanned_anchor),
                    key,
                    "expected a table of [rules.<name>] tables",
                )),
            },
            "severity" => match item.as_table() {
                Some(severity) => {
                    walk_severity(file, severity, configuration, diagnostics, unspanned_anchor);
                }
                None => diagnostics.push(invalid_value(
                    file,
                    item_range(table, key, item, unspanned_anchor),
                    key,
                    "expected a table",
                )),
            },
            "plugins" => match item.as_table() {
                Some(plugins) => {
                    for (plugin_key, _) in plugins.iter() {
                        diagnostics.push(unknown_key(
                            file,
                            key_range(plugins, plugin_key, unspanned_anchor),
                            &format!("plugins.{plugin_key}"),
                        ));
                    }
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

/// The `[rules.<name>]` tables: each is a table with at most one key,
/// `enabled`. Any other key has nowhere to go, since no rule takes
/// options yet.
fn walk_rules(
    file: FileId,
    rules: &toml_edit::Table,
    configuration: &mut Configuration,
    diagnostics: &mut Vec<Diagnostic>,
    fallback: TextRange,
) {
    for (name, item) in rules.iter() {
        let name_range = key_range(rules, name, fallback);
        let Some(rule) = item.as_table() else {
            diagnostics.push(invalid_value(
                file,
                item_range(rules, name, item, fallback),
                &format!("rules.{name}"),
                &format!("expected a table like [rules.{name}]"),
            ));
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
    severity: &toml_edit::Table,
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
        match parsed {
            Some(value) => configuration.severity.push(SeverityEntry {
                identifier: Spanned {
                    value: identifier.to_owned(),
                    range: identifier_range,
                },
                severity: Spanned {
                    value,
                    range: value_range,
                },
            }),
            None => diagnostics.push(invalid_value(
                file,
                value_range,
                &format!("severity.{identifier}"),
                "expected \"error\" or \"warning\"",
            )),
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
        assert_eq!(configuration.severity.len(), 1);
        let entry = configuration.severity.first().unwrap();
        assert_eq!(entry.identifier.value, "CEL0034");
        assert_eq!(
            entry.severity.value,
            celerrate_diagnostics::Severity::Warning
        );
        let diagnostic = single(&diagnostics);
        assert_eq!(diagnostic.id, INVALID_CONFIGURATION_VALUE);
        assert_eq!(
            diagnostic.message,
            "invalid value for `severity.CEL0035`: expected \"error\" or \"warning\"",
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
