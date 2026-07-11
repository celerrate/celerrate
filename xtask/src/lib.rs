//! Development tasks for the Celerrate workspace: `codegen` regenerates
//! the committed sources of `celerrate_syntax` from `php.ungram` and the
//! token table; `fetch-stubs` and `compile-stubs` fetch the pinned
//! phpstorm-stubs snapshot and drive the stub compiler to (re)produce
//! the committed stub blob. xtask deliberately depends on no
//! `celerrate_*` crate: it only spawns `git` and `cargo`, so a broken
//! generated file or blob can never prevent regenerating it.

use std::path::{Path, PathBuf};

pub mod codegen;
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
