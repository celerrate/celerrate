//! The dependency-shape check: plugin crates depend on
//! `celerrate_plugin` and nothing else in the workspace. An extension
//! point that proves insufficient is extended, never bypassed — this
//! check is what makes "never bypassed" mechanical.

/// The plugin crates under the rule. Plan 7 adds the stdlib type
/// provider here.
const PLUGIN_CRATES: &[&str] = &["celerrate_phpdoc_bridge"];
const ALLOWED_DEPENDENCY: &str = "celerrate_plugin";

pub fn run() -> crate::Result<()> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(crate::workspace_root()?)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    check(&metadata)
}

pub(crate) fn check(metadata: &serde_json::Value) -> crate::Result<()> {
    let packages = metadata
        .get("packages")
        .and_then(|value| value.as_array())
        .ok_or("cargo metadata: no packages array")?;
    let mut seen = std::collections::BTreeSet::new();
    for package in packages {
        let Some(name) = package.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        if !PLUGIN_CRATES.contains(&name) {
            continue;
        }
        seen.insert(name.to_owned());
        let Some(dependencies) = package
            .get("dependencies")
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        for dependency in dependencies {
            let Some(dependency_name) = dependency.get("name").and_then(|value| value.as_str())
            else {
                continue;
            };
            // Dev-dependencies are test-only and exempt (recorded in
            // the design decisions); normal and build kinds are not.
            if dependency.get("kind").and_then(|value| value.as_str()) == Some("dev") {
                continue;
            }
            if dependency_name.starts_with("celerrate_") && dependency_name != ALLOWED_DEPENDENCY {
                return Err(format!(
                    "dependency shape violated: {name} depends on {dependency_name}; \
                     plugin crates depend only on {ALLOWED_DEPENDENCY}",
                )
                .into());
            }
        }
    }
    for expected in PLUGIN_CRATES {
        if !seen.contains(*expected) {
            return Err(format!(
                "dependency shape: plugin crate {expected} not found in the workspace"
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(packages: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "packages": packages })
    }

    fn package(name: &str, dependencies: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "name": name, "dependencies": dependencies })
    }

    #[test]
    fn a_clean_shape_passes() {
        let value = metadata(serde_json::json!([
            package(
                "celerrate_phpdoc_bridge",
                serde_json::json!([
                    { "name": "celerrate_plugin", "kind": null },
                    { "name": "celerrate_types", "kind": "dev" },
                ])
            ),
            package(
                "celerrate_cli",
                serde_json::json!([
                    { "name": "celerrate_types", "kind": null },
                ])
            ),
        ]));
        assert!(check(&value).is_ok());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_workspace_dependency_beyond_the_facade_fails() {
        let value = metadata(serde_json::json!([package(
            "celerrate_phpdoc_bridge",
            serde_json::json!([
                { "name": "celerrate_plugin", "kind": null },
                { "name": "celerrate_types", "kind": null },
            ])
        ),]));
        let error = check(&value).unwrap_err().to_string();
        assert!(error.contains("celerrate_phpdoc_bridge"));
        assert!(error.contains("celerrate_types"));
    }

    #[test]
    fn a_missing_plugin_crate_fails() {
        let value = metadata(serde_json::json!([package(
            "celerrate_cli",
            serde_json::json!([])
        ),]));
        assert!(check(&value).is_err());
    }
}
