//! The pure configuration model: what a parsed `celerrate.toml` says,
//! with the span of every user-written value so semantic validation
//! (here and at the composition root) can anchor precise diagnostics.
//!
//! This model is data, not behavior: nothing here reads a file, knows
//! salsa, or sees the rule registry. Part 2 of the sub-project wires
//! its consumption.

use celerrate_diagnostics::Severity;
use celerrate_source::TextRange;

/// A value and the range of its source text in `celerrate.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub value: T,
    pub range: TextRange,
}

/// One `[rules.<name>]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleEntry {
    /// The rule name as written (the table key).
    pub name: Spanned<String>,
    /// The `enabled` key, absent when the table does not set it (a
    /// valid no-op table).
    pub enabled: Option<Spanned<bool>>,
}

/// One `[severity]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeverityEntry {
    /// The identifier as written (existence is checked by `validate`,
    /// not at parse time).
    pub identifier: Spanned<String>,
    /// The severity, absent when the file's value was not a recognized
    /// severity word, which `parse` has already reported: the
    /// identifier is kept so `validate` can still check it.
    pub severity: Option<Spanned<Severity>>,
}

/// A parsed `celerrate.toml`. Every field is optional or empty by
/// default: an empty file is a valid configuration (zero config is the
/// contract; a file only narrows it).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Configuration {
    /// `[project] php = "8.2"`: the version point that collapses the
    /// detected range (consumed in part 2).
    pub php: Option<Spanned<(u8, u8)>>,
    /// `[project] include = [...]`, relative paths.
    pub include: Vec<Spanned<String>>,
    /// `[project] exclude = [...]`, relative paths.
    pub exclude: Vec<Spanned<String>>,
    /// The `[rules.<name>]` tables, in file order.
    pub rules: Vec<RuleEntry>,
    /// The `[severity]` entries, in file order.
    pub severity: Vec<SeverityEntry>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::Configuration;

    #[test]
    fn the_default_configuration_is_empty() {
        let configuration = Configuration::default();
        assert!(configuration.php.is_none());
        assert!(configuration.include.is_empty());
        assert!(configuration.exclude.is_empty());
        assert!(configuration.rules.is_empty());
        assert!(configuration.severity.is_empty());
    }
}
