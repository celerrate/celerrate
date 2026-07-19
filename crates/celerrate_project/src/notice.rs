use celerrate_diagnostics::{DiagnosticId, Severity};

use crate::version::LATEST_STABLE_VERSION;

/// No `composer.json`: the root directory is analyzed with defaults.
pub const MISSING_COMPOSER_MANIFEST: DiagnosticId = DiagnosticId::new("CEL0025");
/// `composer.json` is not a JSON object: defaults are used.
pub const INVALID_COMPOSER_MANIFEST: DiagnosticId = DiagnosticId::new("CEL0026");
/// No PHP version configured: the latest supported stable is assumed.
pub const PHP_VERSION_FALLBACK: DiagnosticId = DiagnosticId::new("CEL0027");
/// A version constraint is unparseable or admits no supported version.
pub const INVALID_PHP_VERSION_CONSTRAINT: DiagnosticId = DiagnosticId::new("CEL0028");
/// `installed.json` is not a JSON object: vendor autoload is skipped.
pub const INVALID_INSTALLED_PACKAGES: DiagnosticId = DiagnosticId::new("CEL0029");
/// `composer.json` exists but could not be read (an IO error other than
/// not-found): defaults are used, as with a missing one, but the cause
/// is named rather than reported as absence.
pub const UNREADABLE_COMPOSER_MANIFEST: DiagnosticId = DiagnosticId::new("CEL0039");
/// `installed.json` exists but could not be read (an IO error other than
/// not-found): vendor autoload is skipped, and the cause is named rather
/// than dropped silently.
pub const UNREADABLE_INSTALLED_PACKAGES: DiagnosticId = DiagnosticId::new("CEL0040");

/// Every identifier this crate allocates, for the registry check at the
/// composition root.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[
    MISSING_COMPOSER_MANIFEST,
    INVALID_COMPOSER_MANIFEST,
    PHP_VERSION_FALLBACK,
    INVALID_PHP_VERSION_CONSTRAINT,
    INVALID_INSTALLED_PACKAGES,
    UNREADABLE_COMPOSER_MANIFEST,
    UNREADABLE_INSTALLED_PACKAGES,
];

/// One discovery finding, structured. The kind stays with this
/// producing crate (the narrowing recorded in the semantic-core spec,
/// section 7); the preview renderer projects it into the shared
/// diagnostic model when part 7 consumes it. Zero-configuration never
/// blocks: every notice is a warning attached to a fallback already
/// taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectNotice {
    MissingComposerManifest,
    InvalidComposerManifest,
    PhpVersionFallback,
    InvalidPhpVersionConstraint {
        constraint: String,
    },
    InvalidInstalledPackages,
    /// `composer.json` is present but could not be read: the IO error,
    /// carried so the message can name it. Distinct from a missing
    /// manifest, which is an ordinary zero-configuration case.
    UnreadableComposerManifest {
        error: String,
    },
    /// `installed.json` is present but could not be read: the IO error,
    /// carried so the message can name it. Distinct from a missing one,
    /// which drops vendor autoload silently and legitimately.
    UnreadableInstalledPackages {
        error: String,
    },
}

impl ProjectNotice {
    pub fn identifier(&self) -> DiagnosticId {
        match self {
            Self::MissingComposerManifest => MISSING_COMPOSER_MANIFEST,
            Self::InvalidComposerManifest => INVALID_COMPOSER_MANIFEST,
            Self::PhpVersionFallback => PHP_VERSION_FALLBACK,
            Self::InvalidPhpVersionConstraint { .. } => INVALID_PHP_VERSION_CONSTRAINT,
            Self::InvalidInstalledPackages => INVALID_INSTALLED_PACKAGES,
            Self::UnreadableComposerManifest { .. } => UNREADABLE_COMPOSER_MANIFEST,
            Self::UnreadableInstalledPackages { .. } => UNREADABLE_INSTALLED_PACKAGES,
        }
    }

    pub fn severity(&self) -> Severity {
        Severity::Warning
    }

    /// The one-sentence rendering. Every notice announces a fallback
    /// already taken, which is why none of them affects the exit code.
    pub fn message(&self) -> String {
        match self {
            // Not "the current directory": `check` takes a project root,
            // and it is analyzed whether or not it is the one the shell
            // happens to be sitting in. What the fallback really does is
            // analyze the whole root, rather than only the directories an
            // autoload section would have declared.
            Self::MissingComposerManifest => {
                "no composer.json found; analyzing the whole project root".to_owned()
            }
            Self::InvalidComposerManifest => {
                "composer.json is not a JSON object; using defaults".to_owned()
            }
            Self::PhpVersionFallback => {
                format!("no PHP version configured; assuming {LATEST_STABLE_VERSION}")
            }
            Self::InvalidPhpVersionConstraint { constraint } => format!(
                "the PHP version constraint `{constraint}` is unusable; assuming {LATEST_STABLE_VERSION}",
            ),
            Self::InvalidInstalledPackages => {
                "installed.json is not a JSON object; vendor autoload is skipped".to_owned()
            }
            Self::UnreadableComposerManifest { error } => {
                format!(
                    "composer.json could not be read ({error}); analyzing the whole project root"
                )
            }
            Self::UnreadableInstalledPackages { error } => {
                format!("installed.json could not be read ({error}); vendor autoload is skipped")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_diagnostics::{DiagnosticId, Severity};

    use super::{ALLOCATED_IDENTIFIERS, ProjectNotice};

    #[test]
    fn identifiers_are_stable() {
        let cases = [
            (ProjectNotice::MissingComposerManifest, "CEL0025"),
            (ProjectNotice::InvalidComposerManifest, "CEL0026"),
            (ProjectNotice::PhpVersionFallback, "CEL0027"),
            (
                ProjectNotice::InvalidPhpVersionConstraint {
                    constraint: ">=9.0".to_owned(),
                },
                "CEL0028",
            ),
            (ProjectNotice::InvalidInstalledPackages, "CEL0029"),
            (
                ProjectNotice::UnreadableComposerManifest {
                    error: "permission denied".to_owned(),
                },
                "CEL0039",
            ),
            (
                ProjectNotice::UnreadableInstalledPackages {
                    error: "permission denied".to_owned(),
                },
                "CEL0040",
            ),
        ];
        for (notice, identifier) in cases {
            assert_eq!(notice.identifier().as_str(), identifier);
            assert_eq!(notice.severity(), Severity::Warning);
            assert!(!notice.message().is_empty());
        }
    }

    #[test]
    fn an_unreadable_notice_names_the_underlying_io_error() {
        let manifest = ProjectNotice::UnreadableComposerManifest {
            error: "permission denied (os error 13)".to_owned(),
        };
        assert!(
            manifest
                .message()
                .contains("permission denied (os error 13)")
        );
        let installed = ProjectNotice::UnreadableInstalledPackages {
            error: "permission denied (os error 13)".to_owned(),
        };
        assert!(
            installed
                .message()
                .contains("permission denied (os error 13)")
        );
    }

    #[test]
    fn the_allocation_list_is_exactly_what_the_notices_use() {
        let used: Vec<DiagnosticId> = [
            ProjectNotice::MissingComposerManifest,
            ProjectNotice::InvalidComposerManifest,
            ProjectNotice::PhpVersionFallback,
            ProjectNotice::InvalidPhpVersionConstraint {
                constraint: String::new(),
            },
            ProjectNotice::InvalidInstalledPackages,
            ProjectNotice::UnreadableComposerManifest {
                error: String::new(),
            },
            ProjectNotice::UnreadableInstalledPackages {
                error: String::new(),
            },
        ]
        .iter()
        .map(ProjectNotice::identifier)
        .collect();
        assert_eq!(used, ALLOCATED_IDENTIFIERS.to_vec());
    }
}
