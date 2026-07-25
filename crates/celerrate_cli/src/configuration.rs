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
//! Part 1 boundary: the parsed configuration is carried and reports its
//! own diagnostics, counting them toward the exit code. As of the
//! composition root's wiring, `include`/`exclude` and the `php` override
//! are consumed by discovery and the walk. No active rule set is built
//! yet, the severity remap is not applied, and the cache digest does not
//! see it: those remain later tasks of this sub-project.

use std::collections::BTreeSet;
use std::path::Path;

use celerrate_config::KnownSets;
use celerrate_diagnostics::{Diagnostic, Severity};
use celerrate_rules::RuleMetadata;
use celerrate_source::FileId;
use celerrate_vfs::Vfs;

/// The loaded `celerrate.toml`: its identity, its text (for the
/// renderer), the parsed model (for part 2), and its diagnostics.
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use celerrate_vfs::Vfs;

    use super::{known_sets, load};

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
}
