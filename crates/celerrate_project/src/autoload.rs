use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use celerrate_vfs::normalize_path;

/// The autoload rules one `composer.json` (or one installed package)
/// declares. Directories and files are kept as declared, relative to
/// the declaring package's root; [`AutoloadRules::walk_roots`]
/// resolves them. Namespace prefixes are retained for the resolution
/// layers of later parts; the walk flattens them away.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoloadRules {
    pub psr4: Vec<NamespaceMapping>,
    pub psr0: Vec<NamespaceMapping>,
    pub classmap: Vec<String>,
    pub files: Vec<String>,
}

/// One PSR-4 or PSR-0 entry: a namespace prefix and the directories
/// that serve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceMapping {
    pub prefix: String,
    pub directories: Vec<String>,
}

impl AutoloadRules {
    /// Reads one `autoload`-shaped JSON object tolerantly: an absent
    /// or mistyped section yields empty rules, never a failure.
    pub fn from_json(value: Option<&serde_json::Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        Self {
            psr4: namespace_mappings(value.get("psr-4")),
            psr0: namespace_mappings(value.get("psr-0")),
            classmap: string_list(value.get("classmap")),
            files: string_list(value.get("files")),
        }
    }

    /// Appends the other rules; used to fold `autoload-dev` into
    /// `autoload` (test code is project code).
    pub fn merged(mut self, other: Self) -> Self {
        self.psr4.extend(other.psr4);
        self.psr0.extend(other.psr0);
        self.classmap.extend(other.classmap);
        self.files.extend(other.files);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.psr4.is_empty()
            && self.psr0.is_empty()
            && self.classmap.is_empty()
            && self.files.is_empty()
    }

    /// Every declared directory and file, resolved against the
    /// declaring root, normalized, deduplicated, and sorted.
    pub fn walk_roots(&self, base: &Path) -> Vec<PathBuf> {
        let mut roots = BTreeSet::new();
        for mapping in self.psr4.iter().chain(&self.psr0) {
            for directory in &mapping.directories {
                roots.insert(normalize_path(Path::new(directory), base));
            }
        }
        for declared in self.classmap.iter().chain(&self.files) {
            roots.insert(normalize_path(Path::new(declared), base));
        }
        roots.into_iter().collect()
    }
}

fn namespace_mappings(value: Option<&serde_json::Value>) -> Vec<NamespaceMapping> {
    let Some(object) = value.and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    object
        .iter()
        .map(|(prefix, directories)| NamespaceMapping {
            prefix: prefix.clone(),
            directories: match directories {
                serde_json::Value::String(directory) => vec![directory.clone()],
                serde_json::Value::Array(entries) => entries
                    .iter()
                    .filter_map(|entry| entry.as_str().map(str::to_owned))
                    .collect(),
                _ => Vec::new(),
            },
        })
        .collect()
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::{AutoloadRules, NamespaceMapping};

    fn rules(json: &str) -> AutoloadRules {
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        AutoloadRules::from_json(Some(&value))
    }

    #[test]
    fn every_section_is_read_with_string_and_array_directory_forms() {
        let rules = rules(
            r#"{
                "psr-4": { "App\\": "src/", "Tools\\": ["tools/a", "tools/b"] },
                "psr-0": { "Legacy_": "legacy/" },
                "classmap": ["database/seeds", "lib/Single.php"],
                "files": ["helpers.php"]
            }"#,
        );
        assert_eq!(
            rules.psr4,
            vec![
                NamespaceMapping {
                    prefix: String::from("App\\"),
                    directories: vec![String::from("src/")],
                },
                NamespaceMapping {
                    prefix: String::from("Tools\\"),
                    directories: vec![String::from("tools/a"), String::from("tools/b")],
                },
            ],
        );
        assert_eq!(
            rules.psr0,
            vec![NamespaceMapping {
                prefix: String::from("Legacy_"),
                directories: vec![String::from("legacy/")],
            }],
        );
        assert_eq!(
            rules.classmap,
            vec![
                String::from("database/seeds"),
                String::from("lib/Single.php")
            ],
        );
        assert_eq!(rules.files, vec![String::from("helpers.php")]);
    }

    #[test]
    fn absent_and_mistyped_sections_yield_empty_rules() {
        assert!(AutoloadRules::from_json(None).is_empty());
        assert!(rules("{}").is_empty());
        assert!(rules(r#"{ "psr-4": "not an object", "classmap": 3 }"#).is_empty());
        assert!(rules(r#"{ "classmap": [1, true] }"#).is_empty());
    }

    #[test]
    fn merging_appends_the_other_rules() {
        let merged = rules(r#"{ "psr-4": { "App\\": "src/" } }"#).merged(rules(
            r#"{ "psr-4": { "Tests\\": "tests/" }, "files": ["dev.php"] }"#,
        ));
        assert_eq!(merged.psr4.len(), 2);
        assert_eq!(merged.files, vec![String::from("dev.php")]);
    }

    #[test]
    fn walk_roots_resolve_normalize_sort_and_deduplicate() {
        let rules = rules(
            r#"{
                "psr-4": { "App\\": "src/", "Again\\": "./src" },
                "classmap": ["lib/Single.php"],
                "files": ["helpers.php"]
            }"#,
        );
        assert_eq!(
            rules.walk_roots(Path::new("/project")),
            vec![
                PathBuf::from("/project/helpers.php"),
                PathBuf::from("/project/lib/Single.php"),
                PathBuf::from("/project/src"),
            ],
        );
    }
}
