use std::collections::BTreeSet;
use std::fs;
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

/// Discovers the project at an absolute `root`, reading
/// `composer.json` and `<vendor>/composer/installed.json` from disk.
/// This is push-time work for the composition root, never called
/// during a query.
pub fn discover(root: &Path) -> ProjectDiscovery {
    let manifest_text = fs::read_to_string(root.join("composer.json")).ok();
    // The manifest decides where the vendor directory lives, so it is
    // parsed once here just to locate `installed.json`; the pure core
    // re-derives everything from the texts.
    let manifest = manifest_text.as_deref().and_then(parse_manifest);
    let vendor = vendor_root(root, manifest.as_ref());
    let installed_text = fs::read_to_string(vendor.join("composer/installed.json")).ok();
    discover_from_sources(root, manifest_text.as_deref(), installed_text.as_deref())
}

/// The pure core of [`discover`]: derives the configuration from the
/// two file texts (`None` = the file does not exist). `root` must be
/// absolute.
pub fn discover_from_sources(
    root: &Path,
    manifest_text: Option<&str>,
    installed_text: Option<&str>,
) -> ProjectDiscovery {
    let mut notices = Vec::new();
    let manifest = match manifest_text {
        None => {
            notices.push(ProjectNotice::MissingComposerManifest);
            None
        }
        Some(text) => match parse_manifest(text) {
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
    let project_walk_roots = match &manifest {
        Some(manifest) if !manifest.autoload.is_empty() => manifest.autoload.walk_roots(root),
        // Zero-configuration never blocks: no declared autoload means
        // the whole root is analyzed.
        _ => vec![normalize_path(root, root)],
    };
    let vendor_walk_roots = match installed_text {
        None => Vec::new(),
        Some(text) => match parse_installed_packages(text, &vendor_root.join("composer")) {
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
        },
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

    use super::{FileOrigin, discover_from_sources};
    use crate::notice::ProjectNotice;
    use crate::version::{PhpVersion, PhpVersionRange};

    const ROOT: &str = "/project";

    #[test]
    fn without_a_manifest_the_root_is_analyzed_with_defaults_and_one_notice() {
        let discovery = discover_from_sources(Path::new(ROOT), None, None);
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
        let discovery = discover_from_sources(Path::new(ROOT), Some("not json"), None);
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
            Some(
                r#"{
                    "require": { "php": "^8.1" },
                    "config": { "platform": { "php": "8.2.7" } }
                }"#,
            ),
            None,
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
            Some(r#"{ "config": { "platform": { "php": "7.4.33" } } }"#),
            None,
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
            Some(
                r#"{
                    "require": { "php": ">=8.2 <8.5" },
                    "config": { "platform": { "php": "eight" } }
                }"#,
            ),
            None,
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
            Some(r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#),
            None,
        );
        assert_eq!(discovery.php_version_range, PhpVersionRange::fallback());
        assert_eq!(discovery.notices, vec![ProjectNotice::PhpVersionFallback]);
    }

    #[test]
    fn an_unsatisfiable_require_constraint_falls_back_with_one_notice() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(r#"{ "require": { "php": "7.4.*" } }"#),
            None,
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
            Some(
                r#"{
                    "require": { "php": "7.4.*" },
                    "config": { "platform": { "php": "eight" } }
                }"#,
            ),
            None,
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
    fn declared_autoload_replaces_the_root_walk_and_vendor_joins_in() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(
                r#"{
                    "require": { "php": "^8.1" },
                    "autoload": { "psr-4": { "App\\": "src/" } },
                    "autoload-dev": { "psr-4": { "Tests\\": "tests/" } }
                }"#,
            ),
            Some(
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
            Some(r#"{ "require": { "php": "^8.1" } }"#),
            None,
        );
        assert_eq!(discovery.project_walk_roots, vec![PathBuf::from(ROOT)]);
    }

    #[test]
    fn the_vendor_directory_override_moves_the_boundary() {
        let discovery = discover_from_sources(
            Path::new(ROOT),
            Some(r#"{ "require": { "php": "^8.1" }, "config": { "vendor-dir": "third-party" } }"#),
            None,
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
            Some(r#"{ "require": { "php": "^8.1" } }"#),
            Some("not json"),
        );
        assert_eq!(discovery.vendor_walk_roots, Vec::<PathBuf>::new());
        assert_eq!(
            discovery.notices,
            vec![ProjectNotice::InvalidInstalledPackages],
        );
    }
}
