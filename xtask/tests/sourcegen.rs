#![allow(clippy::expect_used, clippy::unwrap_used)]

/// The committed generated files match what the generator produces
/// today. Regenerate with `cargo xtask codegen` when this fails.
#[test]
fn generated_sources_are_fresh() {
    let root = xtask::workspace_root().expect("workspace root");
    let artifacts = xtask::codegen::artifacts().expect("generation succeeds");
    assert!(!artifacts.is_empty());
    for artifact in artifacts {
        let path = root.join(&artifact.relative_path);
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            on_disk,
            artifact.text,
            "{} is stale: run `cargo xtask codegen` and commit the result",
            artifact.relative_path.display()
        );
    }
}
