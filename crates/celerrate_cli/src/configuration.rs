//! `celerrate.toml` loading at the composition root: reads the file
//! next to `composer.json`, interns it into the VFS so the renderer can
//! excerpt it, and runs `celerrate_config`'s parse and validate with the
//! known sets only this crate can see (the rule registry and the
//! diagnostic registry).
//!
//! `celerrate_config` takes the known sets as a parameter rather than
//! looking them up, because both registries sit above it in the
//! dependency DAG. This module is the one place that sees both, which
//! is the whole reason it exists here and not there.
//!
//! The parsed configuration is carried and reports its own diagnostics,
//! counting them toward the exit code. This module loads, validates,
//! and derives: `include`/`exclude` and the `php` override are consumed
//! by discovery and the walk, `configuration_digest` keys the
//! persistent cache on the `[rules]` and `[severity]` sections,
//! `rule_overrides` feeds `celerrate_cli::plugins::core_registrations`
//! the `[rules]` activation overrides, and `severity_remap` is what the
//! per-file composition in `celerrate_cli::analysis` applies before
//! persistence. `merge_diagnostics` and `diagnostic_count` are the
//! presentation side: both `check` and `--watch` merge the loaded
//! file's own diagnostics into a presentation copy of the outcome and
//! into the exit code, never into the outcome the persisted verdicts
//! read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use celerrate_config::KnownSets;
use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_rules::RuleMetadata;
use celerrate_source::FileId;
use celerrate_vfs::Vfs;

/// The loaded `celerrate.toml`: its identity, its text (for the
/// renderer), the parsed model (for rule configuration), and its diagnostics.
pub struct LoadedConfiguration {
    pub file: FileId,
    pub text: String,
    pub configuration: celerrate_config::Configuration,
    pub diagnostics: Vec<Diagnostic>,
}

/// Loads `<root>/celerrate.toml`. `None` when the file does not exist:
/// zero config is the contract, and absence is not an event. Every
/// other failure (unreadable, not UTF-8, invalid TOML) is a diagnostic
/// on the file, never a crash.
pub fn load(root: &Path, vfs: &mut Vfs) -> Option<LoadedConfiguration> {
    let path = root.join("celerrate.toml");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            let file = vfs.file_id(&path);
            return Some(unreadable(
                file,
                format!("celerrate.toml could not be read: {error}"),
            ));
        }
    };
    let file = vfs.file_id(&path);
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            return Some(unreadable(
                file,
                "celerrate.toml is not valid UTF-8".to_owned(),
            ));
        }
    };
    let (configuration, mut diagnostics) = celerrate_config::parse(file, &text);
    // The registration data is owned, and `KnownSets` borrows from it:
    // binding it here keeps it alive across the `validate` call and
    // drops it right after, rather than leaking rule names for the
    // lifetime of the process.
    let metadata: Vec<RuleMetadata> = celerrate_rules::core_rules()
        .into_iter()
        .map(|(metadata, _)| metadata)
        .collect();
    diagnostics.extend(celerrate_config::validate(
        file,
        &configuration,
        &known_sets(&metadata),
    ));
    diagnostics.sort();
    Some(LoadedConfiguration {
        file,
        text,
        configuration,
        diagnostics,
    })
}

/// A file that exists and cannot be read as configuration at all: an
/// unreadable file or one that is not UTF-8. It still gets an identity
/// and a diagnostic, so the report names it like any other finding, and
/// the configuration degrades to its default.
fn unreadable(file: FileId, message: String) -> LoadedConfiguration {
    let range = celerrate_source::TextRange::new(
        celerrate_source::TextSize::from(0),
        celerrate_source::TextSize::from(0),
    );
    LoadedConfiguration {
        file,
        text: String::new(),
        configuration: celerrate_config::Configuration::default(),
        diagnostics: vec![Diagnostic::spanned(
            celerrate_config::INVALID_CONFIGURATION,
            Severity::Error,
            file,
            range,
            message,
        )],
    }
}

/// The known sets, from the registries only the composition root sees.
/// Remappable means "an identifier some core rule may emit"; everything
/// registered but not remappable is resilience by construction, and its
/// severity is not the user's to move.
///
/// The rule names borrow from `metadata`, which the caller owns for as
/// long as the sets are used. The identifiers do not need to: a
/// `DiagnosticId` wraps a `&'static str`, so both identifier sets are
/// `'static` regardless of where they were read from.
fn known_sets(metadata: &[RuleMetadata]) -> KnownSets<'_> {
    let mut rule_names = BTreeSet::new();
    let mut remappable_identifiers = BTreeSet::new();
    for rule in metadata {
        rule_names.insert(rule.name.as_str());
        for identifier in &rule.identifiers {
            remappable_identifiers.insert(identifier.id.as_str());
        }
    }
    let registered_identifiers = celerrate_diagnostics::REGISTRY
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    KnownSets {
        rule_names,
        remappable_identifiers,
        registered_identifiers,
    }
}

/// blake3 over the normalized `[rules]` and `[severity]` sections: the
/// active-set-and-severity cache key the sub-project 4 spec reserved a
/// header field for (CLI product spec section 2). The whole sections
/// are digested, not the derived active set, so future rule options
/// join the key with no header change. Normalization: entries sorted,
/// text length-prefixed, sections count-prefixed, spans dropped.
pub fn configuration_digest(configuration: &celerrate_config::Configuration) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    let mut rules: Vec<(&str, u8)> = configuration
        .rules
        .iter()
        .map(|rule| {
            let enabled = match &rule.enabled {
                None => 0u8,
                Some(enabled) if !enabled.value => 1,
                Some(_) => 2,
            };
            (rule.name.value.as_str(), enabled)
        })
        .collect();
    rules.sort_unstable();
    hash_count(&mut hasher, rules.len());
    for (name, enabled) in rules {
        hash_text(&mut hasher, name);
        hasher.update(&[enabled]);
    }
    let mut severity: Vec<(&str, u8)> = configuration
        .severity
        .iter()
        .map(|entry| {
            let level = match &entry.severity {
                None => 0u8,
                Some(severity) if severity.value == celerrate_diagnostics::Severity::Warning => 1,
                Some(_) => 2,
            };
            (entry.identifier.value.as_str(), level)
        })
        .collect();
    severity.sort_unstable();
    hash_count(&mut hasher, severity.len());
    for (identifier, level) in severity {
        hash_text(&mut hasher, identifier);
        hasher.update(&[level]);
    }
    *hasher.finalize().as_bytes()
}

/// The `[rules]` activation overrides: every table that sets `enabled`,
/// by rule name. Unknown names ride along inert (no registration
/// matches them; CEL0046 already reported the typo), and a table
/// without `enabled` configures nothing, both per spec section 3.
pub fn rule_overrides(loaded: Option<&LoadedConfiguration>) -> BTreeMap<String, bool> {
    let Some(loaded) = loaded else {
        return BTreeMap::new();
    };
    loaded
        .configuration
        .rules
        .iter()
        .filter_map(|rule| {
            rule.enabled
                .as_ref()
                .map(|enabled| (rule.name.value.clone(), enabled.value))
        })
        .collect()
}

/// The `[severity]` remap that actually applies: entries naming a
/// remappable identifier, keyed by identifier text. Resilience and
/// unknown entries were already reported (CEL0048/CEL0049) and must
/// not half-apply; an entry whose value failed to parse carries no
/// severity to apply.
pub fn severity_remap(loaded: Option<&LoadedConfiguration>) -> BTreeMap<String, Severity> {
    let Some(loaded) = loaded else {
        return BTreeMap::new();
    };
    let metadata: Vec<RuleMetadata> = celerrate_rules::core_rules()
        .into_iter()
        .map(|(metadata, _)| metadata)
        .collect();
    let known = known_sets(&metadata);
    loaded
        .configuration
        .severity
        .iter()
        .filter(|entry| {
            known
                .remappable_identifiers
                .contains(entry.identifier.value.as_str())
        })
        .filter_map(|entry| {
            entry
                .severity
                .as_ref()
                .map(|severity| (entry.identifier.value.clone(), severity.value))
        })
        .collect()
}

/// Merges the configuration diagnostics into a presentation outcome and
/// answers how many were merged. Presentation and exit-code input only,
/// never cache input: callers keep the analysis outcome pure and merge
/// into a copy.
pub fn merge_diagnostics(
    session: &crate::session::Session,
    outcome: &mut crate::analysis::AnalysisOutcome,
) -> usize {
    let Some(loaded) = &session.loaded_configuration else {
        return 0;
    };
    outcome
        .diagnostics
        .extend(loaded.diagnostics.iter().cloned());
    outcome.diagnostics.sort();
    loaded.diagnostics.len()
}

/// The configuration diagnostics' contribution to the exit code.
pub fn diagnostic_count(session: &crate::session::Session) -> usize {
    session
        .loaded_configuration
        .as_ref()
        .map(|loaded| loaded.diagnostics.len())
        .unwrap_or(0)
}

fn hash_count(hasher: &mut blake3::Hasher, count: usize) {
    hasher.update(&u64::try_from(count).unwrap_or(u64::MAX).to_le_bytes());
}

fn hash_text(hasher: &mut blake3::Hasher, text: &str) {
    hash_count(hasher, text.len());
    hasher.update(text.as_bytes());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use celerrate_vfs::Vfs;

    use super::{configuration_digest, known_sets, load, rule_overrides, severity_remap};

    /// A project root on disk, written into a temporary directory.
    fn root(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            std::fs::write(root.path().join(path), contents).unwrap();
        }
        root
    }

    #[test]
    fn a_project_without_the_file_loads_nothing_at_all() {
        let root = root(&[]);
        let mut vfs = Vfs::default();
        assert!(load(root.path(), &mut vfs).is_none());
    }

    #[test]
    fn a_valid_file_loads_its_content_and_reports_nothing() {
        let root = root(&[(
            "celerrate.toml",
            "[rules.null-dereference]\nenabled = false\n",
        )]);
        let mut vfs = Vfs::default();
        let loaded = load(root.path(), &mut vfs).unwrap();
        assert!(
            loaded.diagnostics.is_empty(),
            "a valid file is silent: {:?}",
            loaded.diagnostics,
        );
        assert_eq!(loaded.configuration.rules.len(), 1);
        assert_eq!(
            vfs.path(loaded.file).map(std::path::Path::to_path_buf),
            Some(root.path().join("celerrate.toml")),
            "the file is interned so the renderer can name and excerpt it",
        );
        assert_eq!(
            loaded.text, "[rules.null-dereference]\nenabled = false\n",
            "the text travels with the load: the VFS never read this file",
        );
    }

    /// The whole point of the composition root doing this: the parse
    /// half cannot know the rule registry, so an unknown rule can only
    /// be caught here.
    #[test]
    fn an_unknown_rule_is_caught_because_the_root_supplies_the_registry() {
        let root = root(&[("celerrate.toml", "[rules.nul-dereference]\n")]);
        let mut vfs = Vfs::default();
        let loaded = load(root.path(), &mut vfs).unwrap();
        assert_eq!(loaded.diagnostics.len(), 1);
        assert_eq!(
            loaded.diagnostics[0].id,
            celerrate_config::UNKNOWN_RULE,
            "{:?}",
            loaded.diagnostics,
        );
    }

    /// Not UTF-8 is not a crash: the file gets an identity, a
    /// diagnostic, and the default configuration.
    #[test]
    fn a_file_that_is_not_utf8_is_a_diagnostic_not_a_failure() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("celerrate.toml"), [0xff, 0xfe, 0x00]).unwrap();
        let mut vfs = Vfs::default();
        let loaded = load(root.path(), &mut vfs).unwrap();
        assert_eq!(loaded.diagnostics.len(), 1);
        assert_eq!(
            loaded.diagnostics[0].id,
            celerrate_config::INVALID_CONFIGURATION,
        );
        assert_eq!(
            loaded.configuration,
            celerrate_config::Configuration::default()
        );
    }

    /// The sets are what `celerrate_config` cannot see for itself: the
    /// registered rule names, and the split between the identifiers a
    /// rule may emit and the resilience ones nobody may remap.
    #[test]
    fn the_known_sets_name_the_core_rules_and_split_the_identifiers() {
        let metadata: Vec<celerrate_rules::RuleMetadata> = celerrate_rules::core_rules()
            .into_iter()
            .map(|(metadata, _)| metadata)
            .collect();
        let known = known_sets(&metadata);
        assert!(known.rule_names.contains("null-dereference"));
        assert!(
            known
                .remappable_identifiers
                .is_subset(&known.registered_identifiers),
            "every identifier a rule may emit is registered",
        );
        assert!(
            known.registered_identifiers.contains("CEL0026")
                && !known.remappable_identifiers.contains("CEL0026"),
            "a resilience identifier is registered and not remappable",
        );
    }

    /// Parses a `celerrate.toml` text into its model, for digest tests.
    fn model_of(text: &str) -> celerrate_config::Configuration {
        let (configuration, _) = celerrate_config::parse(celerrate_source::FileId::new(0), text);
        configuration
    }

    #[test]
    fn the_digest_ignores_span_and_order_but_not_content() {
        let ordered =
            model_of("[rules.a-rule]\nenabled = true\n\n[rules.b-rule]\nenabled = false\n");
        let reversed =
            model_of("[rules.b-rule]\nenabled = false\n\n[rules.a-rule]\nenabled = true\n");
        assert_eq!(
            configuration_digest(&ordered),
            configuration_digest(&reversed),
            "normalization sorts the entries",
        );
        let changed =
            model_of("[rules.a-rule]\nenabled = false\n\n[rules.b-rule]\nenabled = false\n");
        assert_ne!(
            configuration_digest(&ordered),
            configuration_digest(&changed)
        );
    }

    #[test]
    fn a_severity_entry_moves_the_digest() {
        let without = model_of("");
        let with = model_of("[severity]\n\"CEL0034\" = \"warning\"\n");
        assert_ne!(configuration_digest(&without), configuration_digest(&with));
    }

    #[test]
    fn no_file_and_an_empty_file_share_the_digest() {
        assert_eq!(
            configuration_digest(&celerrate_config::Configuration::default()),
            configuration_digest(&model_of("")),
        );
    }

    #[test]
    fn rule_overrides_carry_every_enabled_key_and_nothing_else() {
        let root = root(&[(
            "celerrate.toml",
            "[rules.null-dereference]\nenabled = false\n\n[rules.unknown-members]\n",
        )]);
        let mut vfs = Vfs::default();
        let loaded = load(root.path(), &mut vfs);
        let overrides = rule_overrides(loaded.as_ref());
        assert_eq!(overrides.len(), 1, "a table without `enabled` is a no-op");
        assert_eq!(overrides.get("null-dereference"), Some(&false));
    }

    #[test]
    fn the_remap_keeps_remappable_entries_and_drops_the_reported_ones() {
        let root = root(&[(
            "celerrate.toml",
            "[severity]\n\"CEL0034\" = \"warning\"\n\"CEL0026\" = \"warning\"\n\"CEL9999\" = \"warning\"\n",
        )]);
        let mut vfs = Vfs::default();
        let loaded = load(root.path(), &mut vfs);
        let remap = severity_remap(loaded.as_ref());
        assert_eq!(remap.len(), 1, "resilience and unknown entries never apply");
        assert_eq!(
            remap.get("CEL0034"),
            Some(&celerrate_diagnostics::Severity::Warning),
        );
    }

    #[test]
    fn the_project_table_does_not_move_the_digest() {
        // include/exclude change file membership (per-entry keys) and the
        // php override moves the header's own range fields: neither
        // belongs in this digest, per the spec's normalized-sections rule.
        let without = model_of("");
        let with = model_of("[project]\nphp = \"8.2\"\ninclude = [\"src\"]\n");
        assert_eq!(configuration_digest(&without), configuration_digest(&with));
    }
}
