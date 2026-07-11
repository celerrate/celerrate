//! Development tasks for the Celerrate workspace. The only command today
//! is `codegen`: regenerate the committed sources of `celerrate_syntax`
//! from `php.ungram` and the token table. xtask deliberately depends on
//! no `celerrate_*` crate, so a broken generated file can never prevent
//! regenerating it.

use std::path::{Path, PathBuf};

pub mod codegen;

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
