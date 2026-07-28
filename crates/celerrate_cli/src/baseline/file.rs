//! The `celerrate-baseline.toml` format: a versioned header and
//! deterministically sorted `[[entry]]` tables, for diffs that stay
//! minimal and reviewable in a pull request. Resilient like everything
//! else: what does not parse produces a failure line, never a crash,
//! and valid entries still apply.

use crate::baseline::entry::BaselineEntry;

pub const FORMAT_VERSION: i64 = 1;

const ENTRY_KEYS: [&str; 5] = ["path", "identifier", "symbol", "message", "count"];

const HEADER: &str = "\
# Celerrate baseline: known findings hidden from the report and the exit code.
# Recorded by `celerrate check --baseline`. Entries are structural (no line
# numbers): they survive moving code and die with their finding.
";

pub struct ParsedBaseline {
    pub entries: Vec<BaselineEntry>,
    pub failures: Vec<String>,
}

pub fn parse(text: &str) -> ParsedBaseline {
    // Mirrors `celerrate_config`'s parse entry point (crates/celerrate_config/
    // src/parse.rs) for the toml_edit 0.23 API spelling.
    let document = match toml_edit::Document::parse(text) {
        Ok(document) => document,
        Err(error) => {
            return ParsedBaseline {
                entries: Vec::new(),
                failures: vec![format!("invalid TOML: {error}")],
            };
        }
    };
    match document
        .get("version")
        .and_then(toml_edit::Item::as_integer)
    {
        Some(FORMAT_VERSION) => {}
        Some(other) => {
            return ParsedBaseline {
                entries: Vec::new(),
                failures: vec![format!(
                    "unsupported baseline version {other}; this binary reads version {FORMAT_VERSION}"
                )],
            };
        }
        None => {
            return ParsedBaseline {
                entries: Vec::new(),
                failures: vec!["the `version` key is missing".to_string()],
            };
        }
    }
    let mut entries = Vec::new();
    let mut failures = Vec::new();
    let tables = document
        .get("entry")
        .and_then(toml_edit::Item::as_array_of_tables);
    if let Some(tables) = tables {
        for (index, table) in tables.iter().enumerate() {
            match parse_entry(table) {
                Ok(entry) => entries.push(entry),
                Err(reason) => failures.push(format!("entry {index}: {reason}")),
            }
            for (key, _) in table.iter() {
                if !ENTRY_KEYS.contains(&key) {
                    failures.push(format!("entry {index}: unknown key `{key}`"));
                }
            }
        }
    }
    ParsedBaseline { entries, failures }
}

fn parse_entry(table: &toml_edit::Table) -> Result<BaselineEntry, String> {
    let count = table
        .get("count")
        .and_then(toml_edit::Item::as_integer)
        .ok_or_else(|| "the `count` key is missing or not an integer".to_string())?;
    let count = u32::try_from(count)
        .ok()
        .filter(|count| *count >= 1)
        .ok_or_else(|| format!("the `count` key must be a positive integer, got {count}"))?;
    Ok(BaselineEntry {
        path: required_text(table, "path")?,
        identifier: required_text(table, "identifier")?,
        symbol: required_text(table, "symbol")?,
        message: required_text(table, "message")?,
        count,
    })
}

fn required_text(table: &toml_edit::Table, key: &str) -> Result<String, String> {
    table
        .get(key)
        .and_then(toml_edit::Item::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("the `{key}` key is missing or not a string"))
}

pub fn serialize(entries: &[BaselineEntry]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort();
    let mut document = toml_edit::DocumentMut::new();
    document.insert("version", toml_edit::value(FORMAT_VERSION));
    let mut tables = toml_edit::ArrayOfTables::new();
    for entry in &sorted {
        let mut table = toml_edit::Table::new();
        table.insert("path", toml_edit::value(&entry.path));
        table.insert("identifier", toml_edit::value(&entry.identifier));
        table.insert("symbol", toml_edit::value(&entry.symbol));
        table.insert("message", toml_edit::value(&entry.message));
        table.insert("count", toml_edit::value(i64::from(entry.count)));
        tables.push(table);
    }
    document.insert("entry", toml_edit::Item::ArrayOfTables(tables));
    format!("{HEADER}\n{document}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::baseline::entry::BaselineEntry;

    fn entry(
        path: &str,
        identifier: &str,
        symbol: &str,
        message: &str,
        count: u32,
    ) -> BaselineEntry {
        BaselineEntry {
            path: path.to_string(),
            identifier: identifier.to_string(),
            symbol: symbol.to_string(),
            message: message.to_string(),
            count,
        }
    }

    #[test]
    fn serialize_then_parse_round_trips() {
        let entries = vec![
            entry(
                "src/B.php",
                "CEL0018",
                "App\\B::run",
                "unknown class `Missing`",
                2,
            ),
            entry(
                "src/A.php",
                "CEL0018",
                "(top level)",
                "unknown class `Missing`",
                1,
            ),
        ];
        let text = serialize(&entries);
        let parsed = parse(&text);
        assert!(
            parsed.failures.is_empty(),
            "failures: {:?}",
            parsed.failures
        );
        // Serialization sorts: A.php before B.php.
        assert_eq!(parsed.entries[0].path, "src/A.php");
        assert_eq!(parsed.entries[1].count, 2);
        assert_eq!(parsed.entries.len(), 2);
    }

    #[test]
    fn serialization_is_deterministic_regardless_of_input_order() {
        let forward = vec![
            entry("src/A.php", "CEL0018", "(top level)", "m", 1),
            entry("src/B.php", "CEL0018", "(top level)", "m", 1),
        ];
        let backward: Vec<_> = forward.iter().rev().cloned().collect();
        assert_eq!(serialize(&forward), serialize(&backward));
    }

    #[test]
    fn messages_with_toml_special_characters_round_trip() {
        let entries = vec![entry(
            "src/A.php",
            "CEL0030",
            "App\\A::run",
            "unknown method `save` on `App\\User` with \"quotes\"\nand a newline",
            1,
        )];
        let parsed = parse(&serialize(&entries));
        assert!(
            parsed.failures.is_empty(),
            "failures: {:?}",
            parsed.failures
        );
        assert_eq!(parsed.entries, entries);
    }

    #[test]
    fn invalid_toml_reports_one_failure_and_no_entries() {
        let parsed = parse("version = 1\n[[entry]\n");
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.failures.len(), 1);
        assert!(
            parsed.failures[0].contains("invalid TOML"),
            "was: {}",
            parsed.failures[0]
        );
    }

    #[test]
    fn a_missing_or_unsupported_version_rejects_the_whole_file() {
        let missing = parse("[[entry]]\npath = \"a\"\n");
        assert!(missing.entries.is_empty());
        assert!(
            missing.failures[0].contains("version"),
            "was: {}",
            missing.failures[0]
        );

        let unsupported = parse("version = 2\n");
        assert!(unsupported.entries.is_empty());
        assert!(
            unsupported.failures[0].contains("version 2"),
            "was: {}",
            unsupported.failures[0]
        );
    }

    #[test]
    fn a_malformed_entry_is_reported_and_the_valid_ones_still_apply() {
        let text = "version = 1\n\n[[entry]]\npath = \"src/A.php\"\nidentifier = \"CEL0018\"\nsymbol = \"(top level)\"\nmessage = \"m\"\ncount = 1\n\n[[entry]]\npath = \"src/B.php\"\ncount = 0\n";
        let parsed = parse(text);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].path, "src/A.php");
        assert!(!parsed.failures.is_empty());
    }

    #[test]
    fn an_unknown_key_in_an_entry_is_reported_but_the_entry_still_applies() {
        let text = "version = 1\n\n[[entry]]\npath = \"src/A.php\"\nidentifier = \"CEL0018\"\nsymbol = \"(top level)\"\nmessage = \"m\"\ncount = 1\nline = 12\n";
        let parsed = parse(text);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.failures.len(), 1);
        assert!(
            parsed.failures[0].contains("`line`"),
            "was: {}",
            parsed.failures[0]
        );
    }
}
