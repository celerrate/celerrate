use celerrate_diagnostics::{DiagnosticId, Severity};

/// No `composer.json`: the root directory is analyzed with defaults.
pub const MISSING_COMPOSER_MANIFEST: DiagnosticId = DiagnosticId::new("CEL0018");
/// `composer.json` is not a JSON object: defaults are used.
pub const INVALID_COMPOSER_MANIFEST: DiagnosticId = DiagnosticId::new("CEL0019");
/// No PHP version configured: the latest supported stable is assumed.
pub const PHP_VERSION_FALLBACK: DiagnosticId = DiagnosticId::new("CEL0020");
/// A version constraint is unparseable or admits no supported version.
pub const INVALID_PHP_VERSION_CONSTRAINT: DiagnosticId = DiagnosticId::new("CEL0021");
/// `installed.json` is unreadable: vendor autoload is skipped.
pub const INVALID_INSTALLED_PACKAGES: DiagnosticId = DiagnosticId::new("CEL0022");

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
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_diagnostics::Severity;

    use super::ProjectNotice;

    #[test]
    fn identifiers_are_stable() {
        let cases = [
            (ProjectNotice::MissingComposerManifest, "CEL0018"),
            (ProjectNotice::InvalidComposerManifest, "CEL0019"),
            (ProjectNotice::PhpVersionFallback, "CEL0020"),
            (
                ProjectNotice::InvalidPhpVersionConstraint {
                    constraint: String::from("banana"),
                },
                "CEL0021",
            ),
            (ProjectNotice::InvalidInstalledPackages, "CEL0022"),
        ];
        for (notice, identifier) in cases {
            assert_eq!(notice.identifier().as_str(), identifier);
            assert_eq!(notice.severity(), Severity::Warning);
        }
    }
}
