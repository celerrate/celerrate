//! Development tasks for the Celerrate workspace: `codegen` regenerates
//! the committed sources of `celerrate_syntax` from `php.ungram` and the
//! token table; `fetch-stubs` and `compile-stubs` fetch the pinned
//! phpstorm-stubs snapshot and drive the stub compiler to (re)produce
//! the committed stub blob; `corpus` and `bench` fetch the pinned
//! benchmark corpus and measure it; `fetch-phpdoc-parser` and
//! `phpdoc-cases` fetch the pinned phpstan/phpdoc-parser snapshot and
//! extract its `TypeParserTest` inputs into the committed case file the
//! phpdoc-bridge coverage test consumes. xtask deliberately depends on
//! no `celerrate_*` crate: it only spawns `git`, `cargo`, `composer`,
//! `hyperfine`, and the built `celerrate` binary, so a broken generated
//! file, blob, or build can never prevent regenerating what fixes it.

use std::path::{Path, PathBuf};
use std::process::Command;

pub mod bench;
pub mod codegen;
pub mod corpus;
pub mod dependency_shape;
pub mod phpdoc_corpus;
pub mod pin;
pub mod release;
pub mod stubs;

/// Errors are rendered once, in `main`; no variant needs matching.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// The workspace root: xtask lives one level below it.
pub fn workspace_root() -> Result<PathBuf> {
    let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_directory
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask must live one level below the workspace root".into())
}

/// Builds the release binary and returns its path. Every corpus and
/// benchmark run goes through the optimized build: the numbers and the
/// snapshot must describe what users download, not a debug build.
pub fn release_binary() -> Result<PathBuf> {
    let root = workspace_root()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .current_dir(&root)
        .args(["build", "--release", "--package", "celerrate_cli"])
        .status()?;
    if !status.success() {
        return Err("the release build failed".into());
    }
    Ok(root.join("target/release/celerrate"))
}
