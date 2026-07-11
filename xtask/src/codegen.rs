//! Source generation: `artifacts()` produces every generated file as
//! text (a pure function of `php.ungram` and the token table), `run()`
//! writes them to the workspace. The freshness test compares
//! `artifacts()` against the committed files.

pub mod emit_kinds;
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

/// Every artifact, in a fixed order, rustfmt-formatted.
pub fn artifacts() -> Result<Vec<Artifact>> {
    let text = php_ungram_source()?;
    let grammar = grammar::load(&text)?;
    Ok(vec![Artifact {
        relative_path: PathBuf::from("crates/celerrate_syntax/src/syntax_kind/generated.rs"),
        text: reformat(&emit_kinds::syntax_kind_file(&grammar))?,
    }])
}

/// The raw text of `php.ungram`.
pub fn php_ungram_source() -> Result<String> {
    let path = crate::workspace_root()?.join("crates/celerrate_syntax/php.ungram");
    std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()).into())
}

/// Pipes generated text through rustfmt so the committed artifacts are
/// byte-stable under `cargo fmt --check` and the freshness comparison.
/// The toolchain pin (`rust-toolchain.toml`) fixes the rustfmt version.
fn reformat(text: &str) -> Result<String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("rustfmt is not runnable: {error}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!("rustfmt rejected generated code:\n{text}").into());
    }
    Ok(String::from_utf8(output.stdout)?)
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
