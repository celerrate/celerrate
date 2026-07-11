//! Source generation: `artifacts()` produces every generated file as
//! text (a pure function of `php.ungram` and the token table), `run()`
//! writes them to the workspace. The freshness test compares
//! `artifacts()` against the committed files.

pub mod grammar;
pub mod tokens;

use std::path::PathBuf;

use crate::Result;

/// One generated file: a workspace-relative path and its full text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub relative_path: PathBuf,
    pub text: String,
}

/// Every artifact, in a fixed order. Grows as the emitters land.
pub fn artifacts() -> Result<Vec<Artifact>> {
    Ok(Vec::new())
}

/// The raw text of `php.ungram`.
pub fn php_ungram_source() -> Result<String> {
    let path = crate::workspace_root()?.join("crates/celerrate_syntax/php.ungram");
    std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()).into())
}

/// Writes every artifact into the workspace.
pub fn run() -> Result<()> {
    let root = crate::workspace_root()?;
    for artifact in artifacts()? {
        let path = root.join(&artifact.relative_path);
        std::fs::write(&path, &artifact.text)?;
        println!("wrote {}", artifact.relative_path.display());
    }
    Ok(())
}
