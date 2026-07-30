//! From a `phpstan.neon` entry file to one merged settings value:
//! includes resolved recursively (cycle-guarded, relative to the
//! including file), parameters merged with NEON semantics (lists
//! concatenate in include order, the including file's scalars win),
//! paths rebased onto the project root, and everything that does not
//! carry recorded for the report. Resilient throughout: a missing or
//! unparseable include is a report line, never a failure.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::neon;

/// The merged, root-relative view of a PHPStan configuration tree.
#[derive(Debug, Default)]
pub(crate) struct Settings {
    pub(crate) paths: Vec<String>,
    pub(crate) exclude_paths: Vec<String>,
    pub(crate) level: Option<String>,
    pub(crate) untransposed: Vec<Untransposed>,
    pub(crate) ignored_includes: Vec<String>,
    pub(crate) problems: Vec<String>,
}

/// A key the migration does not carry over, with the file it came from.
#[derive(Debug)]
pub(crate) struct Untransposed {
    pub(crate) key: String,
    pub(crate) origin: String,
}

/// Load and merge a PHPStan configuration tree from its entry file.
/// The only hard failure is an unreadable entry file; everything else
/// degrades into report lines.
pub(crate) fn load(source: &Path, root: &Path) -> Result<Settings, String> {
    let origin = source.file_name().map_or_else(
        || "phpstan.neon".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let text = std::fs::read_to_string(source)
        .map_err(|error| format!("could not read {origin}: {error}"))?;
    let mut settings = Settings::default();
    let mut visited = BTreeSet::new();
    visited.insert(celerrate_vfs::normalize_path(source, root));
    absorb(&text, source, &origin, root, &mut visited, &mut settings);
    Ok(settings)
}

/// Absorb one file: its includes first (NEON merge semantics make the
/// including file win), then its own parameters.
fn absorb(
    text: &str,
    file: &Path,
    origin: &str,
    root: &Path,
    visited: &mut BTreeSet<PathBuf>,
    settings: &mut Settings,
) {
    let parsed = neon::parse(text);
    for skipped in &parsed.skipped {
        settings
            .problems
            .push(format!("{origin}:{}: {}", skipped.line, skipped.reason));
    }
    for (key, value) in &parsed.root {
        if key == "includes" {
            absorb_includes(value, file, origin, root, visited, settings);
        }
    }
    for (key, value) in &parsed.root {
        match key.as_str() {
            "includes" => {}
            "parameters" => absorb_parameters(value, origin, settings),
            _ => note_untransposed(settings, key, origin),
        }
    }
}

fn absorb_includes(
    value: &neon::Value,
    file: &Path,
    origin: &str,
    root: &Path,
    visited: &mut BTreeSet<PathBuf>,
    settings: &mut Settings,
) {
    let neon::Value::List(items) = value else {
        settings
            .problems
            .push(format!("{origin}: `includes` is not a list, skipped"));
        return;
    };
    for item in items {
        let neon::Value::Scalar(target) = item else {
            settings
                .problems
                .push(format!("{origin}: structured include entry skipped"));
            continue;
        };
        if ignored_include(target) {
            settings.ignored_includes.push(target.clone());
            continue;
        }
        let directory = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        let resolved = directory.join(target.replace('\\', "/"));
        let normalized = celerrate_vfs::normalize_path(&resolved, root);
        if !visited.insert(normalized) {
            settings
                .problems
                .push(format!("{origin}: circular include of {target}, skipped"));
            continue;
        }
        let child_origin = join_relative(parent_of(origin), target);
        match std::fs::read_to_string(&resolved) {
            Ok(text) => absorb(&text, &resolved, &child_origin, root, visited, settings),
            Err(error) => settings.problems.push(format!(
                "{origin}: include {target} could not be read: {error}"
            )),
        }
    }
}

/// A PHPStan baseline include or a non-NEON include: listed by name in
/// the report, never parsed (Celerrate re-records the baseline instead
/// of converting it entry by entry).
fn ignored_include(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    !lower.ends_with(".neon") || lower.contains("baseline")
}

fn absorb_parameters(value: &neon::Value, origin: &str, settings: &mut Settings) {
    let neon::Value::Map(entries) = value else {
        settings
            .problems
            .push(format!("{origin}: `parameters` is not a mapping, skipped"));
        return;
    };
    let directory = parent_of(origin).to_owned();
    for (key, value) in entries {
        match key.as_str() {
            "paths" => {
                let mut paths = std::mem::take(&mut settings.paths);
                absorb_path_list(value, &directory, origin, key, &mut paths, settings);
                settings.paths = paths;
            }
            "excludePaths" => {
                let mut excludes = std::mem::take(&mut settings.exclude_paths);
                match value {
                    neon::Value::Map(sections) => {
                        for (section, list) in sections {
                            if section == "analyse" || section == "analyseAndScan" {
                                absorb_path_list(
                                    list,
                                    &directory,
                                    origin,
                                    key,
                                    &mut excludes,
                                    settings,
                                );
                            } else {
                                settings.problems.push(format!(
                                    "{origin}: excludePaths.{section} is not understood, skipped"
                                ));
                            }
                        }
                    }
                    other => {
                        absorb_path_list(other, &directory, origin, key, &mut excludes, settings)
                    }
                }
                settings.exclude_paths = excludes;
            }
            "level" => match value {
                neon::Value::Scalar(level) => settings.level = Some(level.clone()),
                _ => settings
                    .problems
                    .push(format!("{origin}: `level` is not a scalar, skipped")),
            },
            _ => note_untransposed(settings, key, origin),
        }
    }
}

fn absorb_path_list(
    value: &neon::Value,
    directory: &str,
    origin: &str,
    key: &str,
    into: &mut Vec<String>,
    settings: &mut Settings,
) {
    let items: Vec<&neon::Value> = match value {
        neon::Value::List(items) => items.iter().collect(),
        single @ neon::Value::Scalar(_) => vec![single],
        neon::Value::Map(_) => {
            settings
                .problems
                .push(format!("{origin}: `{key}` is not a list, skipped"));
            return;
        }
    };
    for item in items {
        match item {
            neon::Value::Scalar(path) => into.push(rebase(directory, path)),
            _ => settings
                .problems
                .push(format!("{origin}: structured `{key}` entry skipped")),
        }
    }
}

/// PHPStan resolves paths relative to the file that declares them:
/// rebase onto the project root. Absolute and placeholder paths pass
/// through raw so the conversion can drop them with a reason.
fn rebase(directory: &str, path: &str) -> String {
    if path.contains('%') || looks_absolute(path) {
        return path.to_owned();
    }
    join_relative(directory, path)
}

fn looks_absolute(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with('\\')
        || path.chars().nth(1).is_some_and(|second| second == ':')
}

/// The directory part of a root-relative origin, empty at the root.
fn parent_of(origin: &str) -> &str {
    origin
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory)
}

/// Join a root-relative directory and a relative path, collapsing `.`
/// and resolving `..` textually. Leading `..` segments survive; the
/// conversion rules drop such paths later.
fn join_relative(directory: &str, relative: &str) -> String {
    let mut segments: Vec<String> = directory
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .map(str::to_owned)
        .collect();
    for segment in relative.replace('\\', "/").split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|last| last != "..") {
                    segments.pop();
                } else {
                    segments.push("..".to_owned());
                }
            }
            other => segments.push(other.to_owned()),
        }
    }
    segments.join("/")
}

fn note_untransposed(settings: &mut Settings, key: &str, origin: &str) {
    if settings.untransposed.iter().any(|entry| entry.key == key) {
        return;
    }
    settings.untransposed.push(Untransposed {
        key: key.to_owned(),
        origin: origin.to_owned(),
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use std::path::Path;

    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let path = root.path().join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        root
    }

    fn load_from(root: &Path) -> Settings {
        load(&root.join("phpstan.neon"), root).unwrap()
    }

    #[test]
    fn paths_exclude_paths_and_level_are_read() {
        let root = project(&[(
            "phpstan.neon",
            "parameters:\n\tlevel: 5\n\tpaths:\n\t\t- src\n\texcludePaths:\n\t\t- src/Generated\n",
        )]);
        let settings = load_from(root.path());
        assert_eq!(settings.paths, ["src"]);
        assert_eq!(settings.exclude_paths, ["src/Generated"]);
        assert_eq!(settings.level.as_deref(), Some("5"));
        assert!(settings.problems.is_empty(), "{:?}", settings.problems);
    }

    #[test]
    fn includes_merge_with_neon_semantics() {
        // Lists concatenate in include order; the including file's
        // scalar wins last.
        let root = project(&[
            (
                "phpstan.neon",
                "includes:\n\t- build/strict.neon\nparameters:\n\tlevel: 5\n\tpaths:\n\t\t- src\n",
            ),
            (
                "build/strict.neon",
                "parameters:\n\tlevel: 3\n\texcludePaths:\n\t\t- fixtures\n",
            ),
        ]);
        let settings = load_from(root.path());
        assert_eq!(settings.level.as_deref(), Some("5"));
        assert_eq!(settings.paths, ["src"]);
        // Declared in build/strict.neon, so rebased onto the root.
        assert_eq!(settings.exclude_paths, ["build/fixtures"]);
    }

    #[test]
    fn baseline_and_non_neon_includes_are_listed_never_read() {
        let root = project(&[(
            "phpstan.neon",
            "includes:\n\t- phpstan-baseline.neon\n\t- rules.php\nparameters:\n\tlevel: 6\n",
        )]);
        let settings = load_from(root.path());
        assert_eq!(
            settings.ignored_includes,
            ["phpstan-baseline.neon", "rules.php"]
        );
        // Never read: no problem line even though neither file exists.
        assert!(settings.problems.is_empty(), "{:?}", settings.problems);
    }

    #[test]
    fn circular_includes_are_guarded_and_reported() {
        let root = project(&[
            ("phpstan.neon", "includes:\n\t- other.neon\n"),
            (
                "other.neon",
                "includes:\n\t- phpstan.neon\nparameters:\n\tlevel: 2\n",
            ),
        ]);
        let settings = load_from(root.path());
        assert_eq!(settings.level.as_deref(), Some("2"));
        assert_eq!(settings.problems.len(), 1, "{:?}", settings.problems);
        assert!(
            settings.problems[0].contains("circular"),
            "{:?}",
            settings.problems
        );
    }

    #[test]
    fn a_missing_include_is_a_problem_line_not_a_failure() {
        let root = project(&[("phpstan.neon", "includes:\n\t- vanished.neon\n")]);
        let settings = load_from(root.path());
        assert_eq!(settings.problems.len(), 1, "{:?}", settings.problems);
        assert!(
            settings.problems[0].contains("vanished.neon"),
            "{:?}",
            settings.problems
        );
    }

    #[test]
    fn the_exclude_paths_mapping_form_feeds_both_sections() {
        let root = project(&[(
            "phpstan.neon",
            "parameters:\n\texcludePaths:\n\t\tanalyse:\n\t\t\t- one\n\t\tanalyseAndScan:\n\t\t\t- two\n",
        )]);
        let settings = load_from(root.path());
        assert_eq!(settings.exclude_paths, ["one", "two"]);
    }

    #[test]
    fn unknown_keys_are_untransposed_and_deduplicated() {
        let root = project(&[
            (
                "phpstan.neon",
                "includes:\n\t- extra.neon\nparameters:\n\tlevel: 5\n\tbootstrapFiles:\n\t\t- tests/bootstrap.php\nservices:\n\t-\n\t\tclass: App\\Extension\n",
            ),
            (
                "extra.neon",
                "parameters:\n\tbootstrapFiles:\n\t\t- other.php\n",
            ),
        ]);
        let settings = load_from(root.path());
        let keys: Vec<&str> = settings
            .untransposed
            .iter()
            .map(|entry| entry.key.as_str())
            .collect();
        assert_eq!(keys, ["bootstrapFiles", "services"]);
        // First origin wins: the include is absorbed before the
        // including file's own parameters.
        assert_eq!(settings.untransposed[0].origin, "extra.neon");
    }

    #[test]
    fn absolute_and_placeholder_paths_pass_through_raw() {
        // Task 3 drops them with reasons; this layer must not mangle
        // them by rebasing.
        let root = project(&[(
            "phpstan.neon",
            "parameters:\n\tpaths:\n\t\t- /somewhere/absolute\n\t\t- '%rootDir%/../src'\n",
        )]);
        let settings = load_from(root.path());
        assert_eq!(settings.paths, ["/somewhere/absolute", "%rootDir%/../src"]);
    }

    #[test]
    fn a_missing_entry_file_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let error = load(&root.path().join("phpstan.neon"), root.path()).unwrap_err();
        assert!(error.contains("phpstan.neon"), "{error}");
    }
}
