//! The dependency-shape check: the governed set of plugin crates is
//! derived from the workspace, not hardcoded — every package with a
//! non-dev dependency on the `celerrate_plugin` facade is a plugin
//! crate, since depending on the facade is the one thing a plugin
//! cannot avoid doing. A new facade dependent is governed the moment
//! it appears, with no xtask edit needed. Composition roots (which
//! legitimately depend on the facade to register implementations, and
//! on everything else besides) are excluded by an explicit allowlist.
//! Governed crates depend on `celerrate_plugin` and nothing else in the
//! workspace, and certain external crates are forbidden as well,
//! closing the external route to the engine. An extension point that
//! proves insufficient is extended, never bypassed — this check is
//! what makes "never bypassed" mechanical.

/// The composition roots: they depend on the facade to register
/// implementations, and legitimately on everything else — the one
/// literal left, failing loud (a facade dependent that is not listed
/// here is governed as a plugin crate).
const COMPOSITION_ROOTS: &[&str] = &["celerrate_cli"];
/// The first-party plugin crates the derived set must always contain:
/// the sanity guard against renames and derivation bugs alike.
const KNOWN_PLUGIN_CRATES: &[&str] = &["celerrate_phpdoc_bridge", "celerrate_stdlib_provider"];
const ALLOWED_DEPENDENCY: &str = "celerrate_plugin";
/// External crates a plugin crate must not depend on directly: the
/// boundary sealing (issue #61) is only mechanical if the external
/// route to the database handle is closed too.
const FORBIDDEN_EXTERNAL_DEPENDENCIES: &[&str] = &["salsa"];

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

    // First pass: derive the governed set. A package is a plugin crate
    // the moment it has a non-dev dependency on the facade, unless it
    // is a declared composition root.
    let mut governed = std::collections::BTreeSet::new();
    for package in packages {
        let Some(name) = package.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        if COMPOSITION_ROOTS.contains(&name) {
            continue;
        }
        let Some(dependencies) = package
            .get("dependencies")
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        let depends_on_facade = dependencies.iter().any(|dependency| {
            let is_facade =
                dependency.get("name").and_then(|value| value.as_str()) == Some(ALLOWED_DEPENDENCY);
            let is_dev = dependency.get("kind").and_then(|value| value.as_str()) == Some("dev");
            is_facade && !is_dev
        });
        if depends_on_facade {
            governed.insert(name.to_owned());
        }
    }

    // Second pass: the existing two rules, run verbatim over exactly
    // the derived set.
    for package in packages {
        let Some(name) = package.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        if !governed.contains(name) {
            continue;
        }
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
            if FORBIDDEN_EXTERNAL_DEPENDENCIES.contains(&dependency_name) {
                return Err(format!(
                    "dependency shape violated: {name} depends on {dependency_name} directly; \
                     plugin crates reach the engine only through {ALLOWED_DEPENDENCY}",
                )
                .into());
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

    // Sanity guard against renames and derivation bugs alike: each
    // known plugin crate must have survived derivation.
    for expected in KNOWN_PLUGIN_CRATES {
        if !governed.contains(*expected) {
            return Err(format!(
                "dependency shape: plugin crate {expected} was not derived from the workspace \
                 (renamed, or the derivation broke)"
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata_with_packages(packages: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({ "packages": packages })
    }

    fn plugin_package(name: &str, dependencies: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({ "name": name, "dependencies": dependencies })
    }

    fn normal(name: &str) -> serde_json::Value {
        serde_json::json!({ "name": name, "kind": null })
    }

    fn dev(name: &str) -> serde_json::Value {
        serde_json::json!({ "name": name, "kind": "dev" })
    }

    #[test]
    fn a_clean_shape_passes() {
        let value = metadata_with_packages(vec![
            plugin_package(
                "celerrate_phpdoc_bridge",
                vec![normal("celerrate_plugin"), dev("celerrate_types")],
            ),
            plugin_package(
                "celerrate_stdlib_provider",
                vec![normal("celerrate_plugin"), dev("celerrate_types")],
            ),
            plugin_package("celerrate_cli", vec![normal("celerrate_types")]),
        ]);
        assert!(check(&value).is_ok());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_missing_second_plugin_crate_fails_the_not_found_guard() {
        // The "not found" guard protects each named plugin crate from a
        // silent rename or removal — this pins it for the second entry,
        // not just the first.
        let value = metadata_with_packages(vec![plugin_package(
            "celerrate_phpdoc_bridge",
            vec![normal("celerrate_plugin")],
        )]);
        let error = check(&value).unwrap_err().to_string();
        assert!(error.contains("celerrate_stdlib_provider"));
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_workspace_dependency_beyond_the_facade_fails() {
        let value = metadata_with_packages(vec![plugin_package(
            "celerrate_phpdoc_bridge",
            vec![normal("celerrate_plugin"), normal("celerrate_types")],
        )]);
        let error = check(&value).unwrap_err().to_string();
        assert!(error.contains("celerrate_phpdoc_bridge"));
        assert!(error.contains("celerrate_types"));
    }

    #[test]
    fn a_missing_plugin_crate_fails() {
        let value = metadata_with_packages(vec![plugin_package("celerrate_cli", vec![])]);
        assert!(check(&value).is_err());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_direct_salsa_dependency_fails_even_though_it_is_not_a_workspace_crate() {
        // The sealing is only mechanical if the check also closes the
        // external route: a plugin adding salsa directly would recover
        // the database handle the facade hides.
        let value = metadata_with_packages(vec![
            plugin_package(
                "celerrate_phpdoc_bridge",
                vec![normal("celerrate_plugin"), normal("salsa")],
            ),
            plugin_package(
                "celerrate_stdlib_provider",
                vec![normal("celerrate_plugin")],
            ),
        ]);
        let error = check(&value).unwrap_err().to_string();
        assert!(error.contains("celerrate_phpdoc_bridge"));
        assert!(error.contains("salsa"));
    }

    #[test]
    fn a_dev_scoped_salsa_dependency_stays_exempt() {
        let value = metadata_with_packages(vec![
            plugin_package(
                "celerrate_phpdoc_bridge",
                vec![normal("celerrate_plugin"), dev("salsa")],
            ),
            plugin_package(
                "celerrate_stdlib_provider",
                vec![normal("celerrate_plugin")],
            ),
        ]);
        assert!(check(&value).is_ok());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_new_facade_dependent_is_governed_without_an_xtask_edit() {
        // A workspace member depending on celerrate_plugin AND another
        // workspace crate: today it would silently escape (not in the
        // hardcoded list); derived, it must fail the shape check.
        let metadata = metadata_with_packages(vec![
            plugin_package("celerrate_phpdoc_bridge", vec![normal("celerrate_plugin")]),
            plugin_package(
                "celerrate_stdlib_provider",
                vec![normal("celerrate_plugin")],
            ),
            plugin_package(
                "celerrate_future_provider",
                vec![normal("celerrate_plugin"), normal("celerrate_types")],
            ),
        ]);
        let error = check(&metadata).unwrap_err().to_string();
        assert!(error.contains("celerrate_future_provider"));
        assert!(error.contains("celerrate_types"));
    }

    #[test]
    fn a_member_without_a_facade_dependency_is_not_governed() {
        let metadata = metadata_with_packages(vec![
            plugin_package("celerrate_phpdoc_bridge", vec![normal("celerrate_plugin")]),
            plugin_package(
                "celerrate_stdlib_provider",
                vec![normal("celerrate_plugin")],
            ),
            plugin_package("celerrate_syntax", vec![normal("celerrate_source")]),
        ]);
        assert!(check(&metadata).is_ok());
    }

    #[test]
    fn the_composition_root_is_not_governed() {
        let metadata = metadata_with_packages(vec![
            plugin_package("celerrate_phpdoc_bridge", vec![normal("celerrate_plugin")]),
            plugin_package(
                "celerrate_stdlib_provider",
                vec![normal("celerrate_plugin")],
            ),
            plugin_package(
                "celerrate_cli",
                vec![normal("celerrate_plugin"), normal("celerrate_types")],
            ),
        ]);
        assert!(check(&metadata).is_ok());
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn a_missing_known_plugin_crate_fails_the_sanity_guard() {
        let metadata = metadata_with_packages(vec![plugin_package(
            "celerrate_phpdoc_bridge",
            vec![normal("celerrate_plugin")],
        )]);
        let error = check(&metadata).unwrap_err().to_string();
        assert!(error.contains("celerrate_stdlib_provider"));
    }
}
