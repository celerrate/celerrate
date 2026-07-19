use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use celerrate_vfs::normalize_path;

use crate::constraint::{php_version_from_text, version_range_for_constraint};
use crate::installed::parse_installed_packages;
use crate::manifest::{ComposerManifest, parse_manifest};
use crate::notice::ProjectNotice;
use crate::version::PhpVersionRange;

/// Whether a file belongs to the project or to an installed
/// dependency. Vendor is the high-durability tier: at the composition
/// root its inputs are invalidated wholesale only when the lock file
/// changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOrigin {
    Project,
    Vendor,
}

/// Everything zero-configuration discovery derives from a project
/// root: the version range, the walk roots, the vendor boundary, and
/// the notices explaining every fallback taken. Discovery never
/// fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiscovery {
    pub root: PathBuf,
    pub vendor_root: PathBuf,
    pub php_version_range: PhpVersionRange,
    pub project_walk_roots: Vec<PathBuf>,
    pub vendor_walk_roots: Vec<PathBuf>,
    pub notices: Vec<ProjectNotice>,
}

impl ProjectDiscovery {
    /// Project roots then vendor roots, each set sorted.
    pub fn walk_roots(&self) -> Vec<PathBuf> {
        self.project_walk_roots
            .iter()
            .chain(&self.vendor_walk_roots)
            .cloned()
            .collect()
    }

    pub fn classify(&self, path: &Path) -> FileOrigin {
        if path.starts_with(&self.vendor_root) {
            FileOrigin::Vendor
        } else {
            FileOrigin::Project
        }
    }
}

/// The outcome of trying to read one discovery input file. `Absent` and
/// `Unreadable` are deliberately distinct: a not-found file is an
/// ordinary zero-configuration case, an unreadable one (permission
/// denied, a directory in the file's place, any other IO error) is a
/// signal the user must see, because analysis then runs over knowingly
/// wrong inputs and the swallowed error would surface only as a
/// downstream false-positive storm with nothing pointing at the cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSource {
    /// The file was read; its text.
    Present(String),
    /// The file does not exist (`ErrorKind::NotFound`).
    Absent,
    /// The file exists but could not be read; the IO error, named in the
    /// resulting notice.
    Unreadable(String),
}

impl From<&str> for FileSource {
    fn from(text: &str) -> Self {
        Self::Present(text.to_owned())
    }
}

/// Reads one discovery input, distinguishing not-found (an ordinary
/// absence) from any other IO error (a signal). Never fails: an IO
/// error becomes [`FileSource::Unreadable`] carrying its message.
fn read_source(path: &Path) -> FileSource {
    match fs::read_to_string(path) {
        Ok(text) => FileSource::Present(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => FileSource::Absent,
        Err(error) => FileSource::Unreadable(error.to_string()),
    }
}

/// Discovers the project at an absolute `root`, reading
/// `composer.json` and `<vendor>/composer/installed.json` from disk.
/// This is push-time work for the composition root, never called
/// during a query.
pub fn discover(root: &Path) -> ProjectDiscovery {
    let manifest = read_source(&root.join("composer.json"));
    // The manifest decides where the vendor directory lives, so it is
    // parsed once here just to locate `installed.json`; the pure core
    // re-derives everything from the texts.
    let parsed = match &manifest {
        FileSource::Present(text) => parse_manifest(text),
        FileSource::Absent | FileSource::Unreadable(_) => None,
    };
    let vendor = vendor_root(root, parsed.as_ref());
    let installed = read_source(&vendor.join("composer/installed.json"));
    discover_from_sources(root, manifest, installed)
}

/// The pure core of [`discover`]: derives the configuration from the
/// two file sources. `root` must be absolute.
pub fn discover_from_sources(
    root: &Path,
    manifest_source: FileSource,
    installed_source: FileSource,
) -> ProjectDiscovery {
    let mut notices = Vec::new();
    let manifest = match manifest_source {
        FileSource::Absent => {
            notices.push(ProjectNotice::MissingComposerManifest);
            None
        }
        FileSource::Unreadable(error) => {
            notices.push(ProjectNotice::UnreadableComposerManifest { error });
            None
        }
        FileSource::Present(text) => match parse_manifest(&text) {
            None => {
                notices.push(ProjectNotice::InvalidComposerManifest);
                None
            }
            Some(manifest) => Some(manifest),
        },
    };
    let vendor_root = vendor_root(root, manifest.as_ref());
    // A missing or invalid manifest already carries its own notice;
    // the version fallback it implies is not separately reported.
    let php_version_range = match &manifest {
        None => PhpVersionRange::fallback(),
        Some(manifest) => resolve_version_range(manifest, &mut notices),
    };
    // Zero-configuration never blocks: when no autoload is declared,
    // or the declared rules resolve to no walkable root (for example
    // every directory value was mistyped), the whole root is analyzed.
    let project_walk_roots = manifest
        .as_ref()
        .map(|manifest| manifest.autoload.walk_roots(root))
        .filter(|walk_roots| !walk_roots.is_empty())
        .unwrap_or_else(|| vec![normalize_path(root, root)]);
    let vendor_walk_roots = match installed_source {
        FileSource::Absent => Vec::new(),
        FileSource::Unreadable(error) => {
            notices.push(ProjectNotice::UnreadableInstalledPackages { error });
            Vec::new()
        }
        FileSource::Present(text) => {
            match parse_installed_packages(&text, &vendor_root.join("composer")) {
                None => {
                    notices.push(ProjectNotice::InvalidInstalledPackages);
                    Vec::new()
                }
                Some(packages) => {
                    let mut roots = BTreeSet::new();
                    for package in packages {
                        roots.extend(package.autoload.walk_roots(&package.root));
                    }
                    roots.into_iter().collect()
                }
            }
        }
    };
    ProjectDiscovery {
        root: normalize_path(root, root),
        vendor_root,
        php_version_range,
        project_walk_roots,
        vendor_walk_roots,
        notices,
    }
}

fn vendor_root(root: &Path, manifest: Option<&ComposerManifest>) -> PathBuf {
    let declared = manifest
        .and_then(|manifest| manifest.vendor_directory.clone())
        .unwrap_or_else(|| String::from("vendor"));
    normalize_path(Path::new(&declared), root)
}

/// The parent spec's detection precedence, minus its `celerrate.toml`
/// first stage: `config.platform.php` (a point, clamped), then
/// `require.php` as a range, then the latest stable with a warning.
/// An unparseable stage reports itself and falls through: each invalid
/// field carries its own notice, but the plain fallback notice never
/// stacks on a constraint notice — it fires only when no version
/// signal existed at all.
fn resolve_version_range(
    manifest: &ComposerManifest,
    notices: &mut Vec<ProjectNotice>,
) -> PhpVersionRange {
    if let Some(platform) = &manifest.platform_php {
        if let Some(version) = php_version_from_text(platform) {
            return PhpVersionRange::point(version.clamped_to_supported());
        }
        notices.push(ProjectNotice::InvalidPhpVersionConstraint {
            constraint: platform.clone(),
        });
    }
    if let Some(require) = &manifest.require_php {
        if let Some(range) = version_range_for_constraint(require) {
            return range;
        }
        notices.push(ProjectNotice::InvalidPhpVersionConstraint {
            constraint: require.clone(),
        });
        return PhpVersionRange::fallback();
    }
    if manifest.platform_php.is_none() {
        notices.push(ProjectNotice::PhpVersionFallback);
    }
    PhpVersionRange::fallback()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::{FileOrigin, FileSource, discover_from_sources};
    use crate::notice::ProjectNotice;
    use crate::version::{PhpVersion, PhpVersionRange};

    const ROOT: &str = "/project";

    #[test]
    fn without_a_manifest_the_root_is_analyzed_with_defaults_and_one_notice() {
        let discovery =
            discover_from_sources(Path::new(ROOT), FileSource::Absent, FileSource::Absent);
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::MissingComposerManifest]
        );
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
        assert_eq!(discovery.vendor_walk_roots, Vec::<PathBuf>::new());
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(discovery.vendor_root, PathBuf::from("/project/vendor"));
    }

    #[test]
    fn an_invalid_manifest_behaves_like_a_missing_one_with_its_own_notice() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from("not json"),
            FileSource::Absent,
        );
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidComposerManifest]
        );
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
    }

    #[test]
    fn the_platform_version_wins_over_the_require_constraint() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(
                r#"{
                    "require": { "php": "^8.1" },
                    "config": { "platform": { "php": "8.2.7" } }
                }"#,
            ),
            FileSource::Absent,
        );
        assert_eq!(
            discovery.php_version_range,
            PhpVersionRange::point(PhpVersion::new(8, 2)),
        );
        assert_eq!(discovery.notices, Vec::<ProjectNotice>::new());
    }

    #[test]
    fn an_unsupported_platform_version_is_clamped() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(r#"{ "config": { "platform": { "php": "7.4.33" } } }"#),
            FileSource::Absent,
        );
        assert_eq!(
            discovery.php_version_range,
            PhpVersionRange::point(PhpVersion::new(8, 1)),
        );
    }

    #[test]
    fn an_invalid_platform_falls_through_to_the_require_constraint() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(
                r#"{
                    "require": { "php": ">=8.2 <8.5" },
                    "config": { "platform": { "php": "eight" } }
                }"#,
            ),
            FileSource::Absent,
        );
        assert_eq!(
            discovery.php_version_range,
            PhpVersionRange::new(PhpVersion::new(8, 2), PhpVersion::new(8, 4)),
        );
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidPhpVersionConstraint {
                constraint: String::from("eight"),
            }],
        );
    }

    #[test]
    fn no_version_signal_at_all_falls_back_with_one_notice() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#),
            FileSource::Absent,
        );
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(discovery.notices, vec![ProjectNotice::PhpVersionFallback]);
    }

    #[test]
    fn an_unsatisfiable_require_constraint_falls_back_with_one_notice() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(r#"{ "require": { "php": "7.4.*" } }"#),
            FileSource::Absent,
        );
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidPhpVersionConstraint {
                constraint: String::from("7.4.*"),
            }],
        );
    }

    #[test]
    fn each_invalid_version_field_reports_itself_once() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(
                r#"{
                    "require": { "php": "7.4.*" },
                    "config": { "platform": { "php": "eight" } }
                }"#,
            ),
            FileSource::Absent,
        );
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(
            discovery.notices,
            vec![
                ProjectNotice::InvalidPhpVersionConstraint {
                    constraint: String::from("eight"),
                },
                ProjectNotice::InvalidPhpVersionConstraint {
                    constraint: String::from("7.4.*"),
                },
            ],
        );
    }

    #[test]
    fn an_invalid_platform_without_require_reports_once_and_falls_back() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(r#"{ "config": { "platform": { "php": "eight" } } }"#),
            FileSource::Absent,
        );
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidPhpVersionConstraint {
                constraint: String::from("eight"),
            }],
        );
    }

    #[test]
    fn declared_autoload_replaces_the_root_walk_and_vendor_joins_in() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(
                r#"{
                    "require": { "php": "^8.1" },
                    "autoload": { "psr-4": { "App\\": "src/" } },
                    "autoload-dev": { "psr-4": { "Tests\\": "tests/" } }
                }"#,
            ),
            FileSource::from(
                r#"{
                    "packages": [
                        {
                            "name": "acme/library",
                            "install-path": "../acme/library",
                            "autoload": { "psr-4": { "Acme\\": "src/" } }
                        }
                    ]
                }"#,
            ),
        );
        assert_eq!(
            discovery.project_walk_roots,
            vec![
                PathBuf::from("/project/src"),
                PathBuf::from("/project/tests")
            ],
        );
        assert_eq!(
            discovery.vendor_walk_roots,
            vec![PathBuf::from("/project/vendor/acme/library/src")],
        );
        assert_eq!(
            discovery.walk_roots(),
            vec![
                PathBuf::from("/project/src"),
                PathBuf::from("/project/tests"),
                PathBuf::from("/project/vendor/acme/library/src"),
            ],
        );
    }

    #[test]
    fn a_manifest_without_autoload_still_walks_the_root() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(r#"{ "require": { "php": "^8.1" } }"#),
            FileSource::Absent,
        );
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
    }

    #[test]
    fn autoload_that_yields_no_walk_roots_falls_back_to_the_root() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(
                r#"{ "require": { "php": "^8.1" }, "autoload": { "psr-4": { "App\\": 42 } } }"#,
            ),
            FileSource::Absent,
        );
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
    }

    #[test]
    fn the_vendor_directory_override_moves_the_boundary() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(
                r#"{ "require": { "php": "^8.1" }, "config": { "vendor-dir": "third-party" } }"#,
            ),
            FileSource::Absent,
        );
        assert_eq!(discovery.vendor_root, PathBuf::from("/project/third-party"));
        assert_eq!(
            discovery.classify(Path::new("/project/third-party/acme/src/A.php")),
            FileOrigin::Vendor,
        );
        assert_eq!(
            discovery.classify(Path::new("/project/src/A.php")),
            FileOrigin::Project,
        );
    }

    #[test]
    fn invalid_installed_packages_skip_vendor_with_a_notice() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(r#"{ "require": { "php": "^8.1" } }"#),
            FileSource::from("not json"),
        );
        assert_eq!(discovery.vendor_walk_roots, Vec::<PathBuf>::new());
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidInstalledPackages],
        );
    }

    #[test]
    fn an_unreadable_manifest_reports_itself_not_a_missing_one() {
        // A present-but-unreadable composer.json falls back like a missing
        // one (whole root, fallback version) but must name the IO error
        // instead of claiming the file does not exist.
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::Unreadable("permission denied (os error 13)".to_owned()),
            FileSource::Absent,
        );
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::UnreadableComposerManifest {
                error: "permission denied (os error 13)".to_owned(),
            }],
        );
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
    }

    #[test]
    fn an_unreadable_installed_json_is_signalled_not_dropped_silently() {
        // A present-but-unreadable installed.json drops every vendor
        // autoload root; today that happened with no signal at all, which
        // reads downstream as a CEL0018 false-positive storm with nothing
        // pointing at the cause.
        let discovery = discover_from_sources(
            Path::new(ROOT),
            FileSource::from(r#"{ "require": { "php": "^8.1" } }"#),
            FileSource::Unreadable("permission denied (os error 13)".to_owned()),
        );
        assert_eq!(discovery.vendor_walk_roots, Vec::<PathBuf>::new());
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::UnreadableInstalledPackages {
                error: "permission denied (os error 13)".to_owned(),
            }],
        );
    }
}
