use std::path::{Path, PathBuf};

use celerrate_vfs::normalize_path;

use crate::autoload::AutoloadRules;

/// One installed dependency: its name, its resolved package root, and
/// the autoload rules it declares. All listed packages count,
/// including dev packages: a symbol declared anywhere in vendor is
/// declared (spec section 7's conservative stance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorPackage {
    pub name: String,
    /// Absolute, normalized package root.
    pub root: PathBuf,
    pub autoload: AutoloadRules,
}

/// Parses `installed.json`. `composer_directory` is the directory
/// containing the file (`<vendor>/composer`), which `install-path`
/// entries are relative to. Accepts the Composer 2 object form
/// (`{"packages": [...]}`) and the Composer 1 bare-array form; `None`
/// when the text is valid in neither shape. Entries without a name
/// are skipped, never failures.
pub fn parse_installed_packages(
    text: &str,
    composer_directory: &Path,
) -> Option<Vec<VendorPackage>> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let packages = match &value {
        serde_json::Value::Array(entries) => entries,
        serde_json::Value::Object(object) => object.get("packages")?.as_array()?,
        _ => return None,
    };
    Some(
        packages
            .iter()
            .filter_map(|package| vendor_package(package, composer_directory))
            .collect(),
    )
}

fn vendor_package(package: &serde_json::Value, composer_directory: &Path) -> Option<VendorPackage> {
    let name = package.get("name")?.as_str()?.to_owned();
    // Composer 1 entries carry no `install-path`; the package then
    // lives at `<vendor>/<name>`, one level above `composer/`.
    let relative_root = package
        .get("install-path")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("../{name}"));
    Some(VendorPackage {
        root: normalize_path(Path::new(&relative_root), composer_directory),
        autoload: AutoloadRules::from_json(package.get("autoload")),
        name,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::parse_installed_packages;

    const COMPOSER_DIRECTORY: &str = "/project/vendor/composer";

    #[test]
    fn composer_2_packages_resolve_their_install_paths() {
        let packages = parse_installed_packages(
            r#"{
                "packages": [
                    {
                        "name": "acme/library",
                        "install-path": "../acme/library",
                        "autoload": { "psr-4": { "Acme\\": "src/" } }
                    }
                ],
                "dev": true,
                "dev-package-names": []
            }"#,
            Path::new(COMPOSER_DIRECTORY),
        )
        .unwrap();
        assert_eq!(packages.len(), 1);
        let package = packages.first().unwrap();
        assert_eq!(package.name, "acme/library");
        assert_eq!(package.root, PathBuf::from("/project/vendor/acme/library"));
        assert_eq!(
            package.autoload.walk_roots(&package.root),
            vec![PathBuf::from("/project/vendor/acme/library/src")],
        );
    }

    #[test]
    fn a_missing_install_path_defaults_to_the_vendor_slash_name_layout() {
        let packages = parse_installed_packages(
            r#"{ "packages": [ { "name": "acme/library" } ] }"#,
            Path::new(COMPOSER_DIRECTORY),
        )
        .unwrap();
        assert_eq!(
            packages.first().unwrap().root,
            PathBuf::from("/project/vendor/acme/library"),
        );
    }

    #[test]
    fn the_composer_1_bare_array_form_is_accepted() {
        let packages = parse_installed_packages(
            r#"[ { "name": "acme/library", "autoload": { "files": ["functions.php"] } } ]"#,
            Path::new(COMPOSER_DIRECTORY),
        )
        .unwrap();
        assert_eq!(
            packages.first().unwrap().root,
            PathBuf::from("/project/vendor/acme/library"),
        );
    }

    #[test]
    fn nameless_entries_are_skipped_and_broken_documents_rejected() {
        let packages = parse_installed_packages(
            r#"{ "packages": [ { "install-path": "../x/y" }, "not an object", { "name": "kept/one" } ] }"#,
            Path::new(COMPOSER_DIRECTORY),
        )
        .unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages.first().unwrap().name, "kept/one");

        assert!(parse_installed_packages("not json", Path::new(COMPOSER_DIRECTORY)).is_none());
        assert!(parse_installed_packages("3", Path::new(COMPOSER_DIRECTORY)).is_none());
        assert!(
            parse_installed_packages(r#"{ "packages": 3 }"#, Path::new(COMPOSER_DIRECTORY))
                .is_none()
        );
    }
}
