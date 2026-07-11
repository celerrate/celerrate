use crate::autoload::AutoloadRules;

/// The fields of `composer.json` the engine consumes, read
/// tolerantly: an absent or mistyped field falls back to its default,
/// never a failure. `autoload` and `autoload-dev` arrive merged (test
/// code is project code).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposerManifest {
    /// `config.platform.php`: the concrete runtime the project pins.
    pub platform_php: Option<String>,
    /// `require.php`: the constraint the project declares.
    pub require_php: Option<String>,
    /// `config.vendor-dir`: where installed dependencies live
    /// (default `vendor`).
    pub vendor_directory: Option<String>,
    pub autoload: AutoloadRules,
}

/// Parses `composer.json`. `None` only when the text is not a JSON
/// object at all; the caller reports the notice and falls back.
pub fn parse_manifest(text: &str) -> Option<ComposerManifest> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    let config = object.get("config");
    Some(ComposerManifest {
        platform_php: config
            .and_then(|config| config.get("platform"))
            .and_then(|platform| platform.get("php"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        require_php: object
            .get("require")
            .and_then(|require| require.get("php"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        vendor_directory: config
            .and_then(|config| config.get("vendor-dir"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        autoload: AutoloadRules::from_json(object.get("autoload"))
            .merged(AutoloadRules::from_json(object.get("autoload-dev"))),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::parse_manifest;

    #[test]
    fn the_consumed_fields_are_extracted() {
        let manifest = parse_manifest(
            r#"{
                "require": { "php": "^8.1", "acme/library": "^2.0" },
                "config": { "platform": { "php": "8.1.2" }, "vendor-dir": "third-party" },
                "autoload": { "psr-4": { "App\\": "src/" } },
                "autoload-dev": { "psr-4": { "Tests\\": "tests/" } }
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.require_php.as_deref(), Some("^8.1"));
        assert_eq!(manifest.platform_php.as_deref(), Some("8.1.2"));
        assert_eq!(manifest.vendor_directory.as_deref(), Some("third-party"));
        let prefixes: Vec<&str> = manifest
            .autoload
            .psr4
            .iter()
            .map(|mapping| mapping.prefix.as_str())
            .collect();
        assert_eq!(prefixes, vec!["App\\", "Tests\\"]);
    }

    #[test]
    fn absent_and_mistyped_fields_fall_back_to_defaults() {
        let manifest =
            parse_manifest(r#"{ "require": { "php": 8 }, "config": "nope", "autoload": [] }"#)
                .unwrap();
        assert_eq!(manifest.require_php, None);
        assert_eq!(manifest.platform_php, None);
        assert_eq!(manifest.vendor_directory, None);
        assert!(manifest.autoload.is_empty());
        assert!(parse_manifest("{}").unwrap().autoload.is_empty());
    }

    #[test]
    fn non_object_documents_are_rejected() {
        assert_eq!(parse_manifest("not json at all"), None);
        assert_eq!(parse_manifest("[1, 2]"), None);
        assert_eq!(parse_manifest("\"just a string\""), None);
    }
}
