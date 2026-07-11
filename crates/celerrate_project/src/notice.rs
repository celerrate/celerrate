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
/// `installed.json` is unreadable: vendor autoload is skipped.
pub const INVALID_INSTALLED_PACKAGES: DiagnosticId = DiagnosticId::new("CEL0029");

/// Every identifier this crate allocates, for the registry check at the
/// composition root.
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] = &[
    MISSING_COMPOSER_MANIFEST,
    INVALID_COMPOSER_MANIFEST,
    PHP_VERSION_FALLBACK,
    INVALID_PHP_VERSION_CONSTRAINT,
    INVALID_INSTALLED_PACKAGES,
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
    InvalidPhpVersionConstraint { constraint: String },
    InvalidInstalledPackages,
}

impl ProjectNotice {
    pub fn identifier(&self) -> DiagnosticId {
        match self {
            Self::MissingComposerManifest => MISSING_COMPOSER_MANIFEST,
            Self::InvalidComposerManifest => INVALID_COMPOSER_MANIFEST,
            Self::PhpVersionFallback => PHP_VERSION_FALLBACK,
            Self::InvalidPhpVersionConstraint { .. } => INVALID_PHP_VERSION_CONSTRAINT,
            Self::InvalidInstalledPackages => INVALID_INSTALLED_PACKAGES,
        }
    }

    pub fn severity(&self) -> Severity {
        Severity::Warning
    }

    /// The one-sentence rendering. Every notice announces a fallback
    /// already taken, which is why none of them affects the exit code.
    pub fn message(&self) -> String {
        match self {
            Self::MissingComposerManifest => {
                "no composer.json found; analyzing the current directory".to_owned()
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
                "installed.json is unreadable; vendor autoload is skipped".to_owned()
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
        ];
        for (notice, identifier) in cases {
            assert_eq!(notice.identifier().as_str(), identifier);
            assert_eq!(notice.severity(), Severity::Warning);
            assert!(!notice.message().is_empty());
        }
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
        ]
        .iter()
        .map(ProjectNotice::identifier)
        .collect();
        assert_eq!(used, ALLOCATED_IDENTIFIERS.to_vec());
    }
}
