//! A minimal NEON reader. NEON is the YAML-like dialect `phpstan.neon`
//! is written in; no mature Rust crate exists, and the migration
//! consumes only a small subset: `includes`, `parameters.paths`,
//! `parameters.excludePaths`, `parameters.level`. This reader parses
//! indentation-structured mappings, `- ` sequences, and inline `[...]`
//! lists into a generic value tree. It is total: every line it does
//! not understand becomes a `Skipped` entry, never a failure.

/// A NEON value, restricted to what the migration consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Scalar(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

/// A line the subset reader did not understand: skipped and reported,
/// never fatal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Skipped {
    /// One-based line number in the source file.
    pub(crate) line: usize,
    pub(crate) reason: &'static str,
}

/// A parsed document: the root mapping plus every skipped line.
#[derive(Debug, Default)]
pub(crate) struct Parsed {
    pub(crate) root: Vec<(String, Value)>,
    pub(crate) skipped: Vec<Skipped>,
}

/// A significant line: comment-stripped, indentation measured in raw
/// leading characters (tabs and spaces alike).
struct Line {
    number: usize,
    indent: usize,
    content: String,
}

/// Parse a NEON document. Total: unknown constructs become `skipped`
/// entries and the rest of the document still parses.
pub(crate) fn parse(text: &str) -> Parsed {
    let lines = significant_lines(text);
    let mut skipped = Vec::new();
    let mut cursor = 0;
    let indent = lines.first().map_or(0, |line| line.indent);
    let root = parse_map(&lines, &mut cursor, indent, &mut skipped);
    while let Some(line) = lines.get(cursor) {
        skipped.push(Skipped {
            line: line.number,
            reason: "unrecognized structure",
        });
        cursor += 1;
    }
    Parsed { root, skipped }
}

fn significant_lines(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let stripped = strip_comment(raw).trim_end();
        let content = stripped.trim_start();
        if content.is_empty() {
            continue;
        }
        lines.push(Line {
            number: index + 1,
            indent: stripped.len() - content.len(),
            content: content.to_owned(),
        });
    }
    lines
}

/// Cut the line at the first `#` that sits outside quotes.
fn strip_comment(raw: &str) -> &str {
    let mut single = false;
    let mut double = false;
    for (index, character) in raw.char_indices() {
        match character {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '#' if !single && !double => return raw.get(..index).unwrap_or(raw),
            _ => {}
        }
    }
    raw
}

fn parse_map(
    lines: &[Line],
    cursor: &mut usize,
    indent: usize,
    skipped: &mut Vec<Skipped>,
) -> Vec<(String, Value)> {
    let mut entries = Vec::new();
    while let Some(line) = lines.get(*cursor) {
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            skipped.push(Skipped {
                line: line.number,
                reason: "unexpected indentation",
            });
            *cursor += 1;
            continue;
        }
        let Some((key, rest)) = split_key(&line.content) else {
            skipped.push(Skipped {
                line: line.number,
                reason: "expected `key: value`",
            });
            *cursor += 1;
            continue;
        };
        let number = line.number;
        *cursor += 1;
        let value = if rest.is_empty() {
            parse_block(lines, cursor, indent, skipped)
        } else {
            inline_value(&rest, number, skipped)
        };
        entries.push((key, value));
    }
    entries
}

/// Parse the child block that follows a `key:` (or bare `-`) line: a
/// sequence or a mapping, decided by the first deeper line. No deeper
/// line means the value was an empty scalar.
fn parse_block(
    lines: &[Line],
    cursor: &mut usize,
    parent_indent: usize,
    skipped: &mut Vec<Skipped>,
) -> Value {
    match lines.get(*cursor) {
        Some(child) if child.indent > parent_indent => {
            let indent = child.indent;
            if child.content.starts_with('-') {
                Value::List(parse_list(lines, cursor, indent, skipped))
            } else {
                Value::Map(parse_map(lines, cursor, indent, skipped))
            }
        }
        _ => Value::Scalar(String::new()),
    }
}

fn parse_list(
    lines: &[Line],
    cursor: &mut usize,
    indent: usize,
    skipped: &mut Vec<Skipped>,
) -> Vec<Value> {
    let mut items = Vec::new();
    while let Some(line) = lines.get(*cursor) {
        if line.indent < indent {
            break;
        }
        let entry = (line.indent == indent)
            .then(|| line.content.strip_prefix('-'))
            .flatten();
        let Some(rest) = entry else {
            skipped.push(Skipped {
                line: line.number,
                reason: "expected a `- item` entry",
            });
            *cursor += 1;
            continue;
        };
        let rest = rest.trim_start().to_owned();
        let number = line.number;
        *cursor += 1;
        if rest.is_empty() {
            items.push(parse_block(lines, cursor, indent, skipped));
        } else if rest.starts_with('{') {
            // A bare inline mapping as a whole list item (`- {a: b}`) is
            // outside the subset; without this check the `:` inside the
            // braces would be mistaken for a `key: value` separator.
            skipped.push(Skipped {
                line: number,
                reason: "inline structures beyond `[a, b]` are outside the subset",
            });
        } else if let Some((key, value_text)) = split_key(&rest) {
            let value = if value_text.is_empty() {
                parse_block(lines, cursor, indent, skipped)
            } else {
                inline_value(&value_text, number, skipped)
            };
            items.push(Value::Map(vec![(key, value)]));
        } else {
            items.push(Value::Scalar(unquote(&rest)));
        }
    }
    items
}

/// Split `key: rest` at the first `:` that sits outside quotes and is
/// followed by whitespace or the end of the line (so `C:/tmp` stays a
/// scalar). The returned key is unquoted, the rest is trimmed.
fn split_key(content: &str) -> Option<(String, String)> {
    let mut single = false;
    let mut double = false;
    for (index, character) in content.char_indices() {
        match character {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            ':' if !single && !double => {
                let after = content.get(index + 1..)?;
                if after.is_empty() || after.starts_with(char::is_whitespace) {
                    let key = unquote(content.get(..index)?.trim());
                    if key.is_empty() {
                        return None;
                    }
                    return Some((key, after.trim().to_owned()));
                }
            }
            _ => {}
        }
    }
    None
}

/// An inline value after `key: `: an `[...]` list, a scalar, or (for
/// `{...}` mappings, outside the subset) a reported skip.
fn inline_value(text: &str, line: usize, skipped: &mut Vec<Skipped>) -> Value {
    if let Some(inner) = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return Value::List(
            split_inline_items(inner)
                .into_iter()
                .map(|item| Value::Scalar(unquote(item)))
                .collect(),
        );
    }
    if text.starts_with('{') || text.starts_with('[') {
        skipped.push(Skipped {
            line,
            reason: "inline structures beyond `[a, b]` are outside the subset",
        });
        return Value::Scalar(String::new());
    }
    Value::Scalar(unquote(text))
}

/// Split the inside of an inline list on commas that sit outside quotes.
fn split_inline_items(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut single = false;
    let mut double = false;
    for (index, character) in inner.char_indices() {
        match character {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            ',' if !single && !double => {
                if let Some(item) = inner.get(start..index) {
                    items.push(item);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if let Some(item) = inner.get(start..) {
        items.push(item);
    }
    items
        .into_iter()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

/// Strip one matching pair of quotes. Double-quoted strings unescape
/// `\"` and `\\` (the two escapes the subset needs); single quotes are
/// literal, NEON-style.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(inner) = trimmed
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return inner.to_owned();
    }
    if let Some(inner) = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return inner.replace("\\\"", "\"").replace("\\\\", "\\");
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use super::*;

    fn root_value<'parsed>(parsed: &'parsed Parsed, key: &str) -> &'parsed Value {
        parsed
            .root
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
            .unwrap()
    }

    #[test]
    fn a_tab_indented_phpstan_file_parses() {
        let parsed = parse(
            "includes:\n\t- phpstan-baseline.neon\n\nparameters:\n\tlevel: 5\n\tpaths:\n\t\t- src\n\t\t- tests\n",
        );
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::List(includes) = root_value(&parsed, "includes") else {
            panic!("includes should be a list");
        };
        assert_eq!(
            includes,
            &[Value::Scalar("phpstan-baseline.neon".to_owned())]
        );
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(
            parameters[0],
            ("level".to_owned(), Value::Scalar("5".to_owned()))
        );
        let Value::List(paths) = &parameters[1].1 else {
            panic!("paths should be a list");
        };
        assert_eq!(
            paths,
            &[
                Value::Scalar("src".to_owned()),
                Value::Scalar("tests".to_owned())
            ]
        );
    }

    #[test]
    fn space_indentation_and_comments_parse_alike() {
        let parsed = parse("parameters:\n    # tuning\n    level: 8 # strict\n");
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(
            parameters[0],
            ("level".to_owned(), Value::Scalar("8".to_owned()))
        );
    }

    #[test]
    fn inline_lists_and_quoted_scalars_parse() {
        let parsed = parse("parameters:\n\tpaths: [src, \"app dir\", 'lib']\n\tlevel: \"max\"\n");
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::List(paths) = &parameters[0].1 else {
            panic!("paths should be a list");
        };
        assert_eq!(
            paths,
            &[
                Value::Scalar("src".to_owned()),
                Value::Scalar("app dir".to_owned()),
                Value::Scalar("lib".to_owned()),
            ]
        );
        assert_eq!(parameters[1].1, Value::Scalar("max".to_owned()));
    }

    #[test]
    fn a_hash_inside_quotes_is_not_a_comment() {
        let parsed = parse("parameters:\n\tignoreErrors:\n\t\t- '#^Call to undefined#'\n");
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::List(ignores) = &parameters[0].1 else {
            panic!("ignoreErrors should be a list");
        };
        assert_eq!(ignores, &[Value::Scalar("#^Call to undefined#".to_owned())]);
    }

    #[test]
    fn a_dash_item_with_a_nested_block_parses_as_a_map() {
        let parsed = parse(
            "parameters:\n\tignoreErrors:\n\t\t-\n\t\t\tmessage: '#unused#'\n\t\t\tpath: src/Legacy.php\n",
        );
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::List(ignores) = &parameters[0].1 else {
            panic!("ignoreErrors should be a list");
        };
        let Value::Map(entry) = &ignores[0] else {
            panic!("the entry should be a map");
        };
        assert_eq!(
            entry[0],
            ("message".to_owned(), Value::Scalar("#unused#".to_owned()))
        );
        assert_eq!(
            entry[1],
            (
                "path".to_owned(),
                Value::Scalar("src/Legacy.php".to_owned())
            )
        );
    }

    #[test]
    fn a_dash_item_with_an_inline_key_parses_as_a_one_entry_map() {
        let parsed = parse("parameters:\n\texcludePaths:\n\t\t- analyse: src/Generated\n");
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::List(excludes) = &parameters[0].1 else {
            panic!("excludePaths should be a list");
        };
        assert_eq!(
            excludes[0],
            Value::Map(vec![(
                "analyse".to_owned(),
                Value::Scalar("src/Generated".to_owned())
            )])
        );
    }

    #[test]
    fn the_exclude_paths_mapping_form_parses() {
        let parsed = parse("parameters:\n\texcludePaths:\n\t\tanalyse:\n\t\t\t- src/Generated\n");
        assert!(parsed.skipped.is_empty(), "{:?}", parsed.skipped);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        let Value::Map(sections) = &parameters[0].1 else {
            panic!("excludePaths should be a map");
        };
        let Value::List(analyse) = &sections[0].1 else {
            panic!("analyse should be a list");
        };
        assert_eq!(analyse, &[Value::Scalar("src/Generated".to_owned())]);
    }

    #[test]
    fn a_colon_inside_a_value_is_not_a_key_separator() {
        // A separator needs trailing whitespace or the end of the line.
        let parsed = parse("parameters:\n\ttmpDir: C:/tmp/phpstan\n");
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(
            parameters[0],
            (
                "tmpDir".to_owned(),
                Value::Scalar("C:/tmp/phpstan".to_owned())
            )
        );
    }

    #[test]
    fn an_empty_value_with_no_child_block_is_an_empty_scalar() {
        let parsed = parse("parameters:\n\tlevel:\n");
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(
            parameters[0],
            ("level".to_owned(), Value::Scalar(String::new()))
        );
    }

    #[test]
    fn constructs_outside_the_subset_are_skipped_with_line_numbers() {
        let parsed = parse("services:\n\t- {factory: App\\Rule}\nparameters:\n\tlevel: 5\n");
        // The inline mapping on line 2 is outside the subset; the rest
        // of the document still parses.
        assert_eq!(parsed.skipped.len(), 1, "{:?}", parsed.skipped);
        assert_eq!(parsed.skipped[0].line, 2);
        let Value::Map(parameters) = root_value(&parsed, "parameters") else {
            panic!("parameters should be a map");
        };
        assert_eq!(
            parameters[0],
            ("level".to_owned(), Value::Scalar("5".to_owned()))
        );
    }

    #[test]
    fn garbage_never_panics() {
        for garbage in [
            "",
            "\n\n\n",
            ":",
            "- - -",
            "\t\tdeep: orphan\n",
            "key with no colon\n",
            "a: [unclosed\n",
            "a: {inline: map}\n",
            "\u{0}\u{1}\u{2}",
            "key:\n\t- \"unterminated\n",
        ] {
            let _ = parse(garbage);
        }
    }
}
