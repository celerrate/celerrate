//! The stub compiler: pinned snapshot in, committed blob out. Driven
//! by `cargo xtask compile-stubs`, never by a build script. Malformed
//! stub files produce warnings and partial extraction; only a missing
//! snapshot or an unwritable output fails the run.

use std::path::PathBuf;
use std::process::ExitCode;

use celerrate_stubs::compiler::extract::extract;
use celerrate_stubs::compiler::snapshot::stub_files;
use celerrate_stubs::{StubIndex, encode};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let (snapshot, output, check) = match (
        arguments.next(),
        arguments.next(),
        arguments.next().as_deref(),
        arguments.next(),
    ) {
        (Some(snapshot), Some(output), None, None) => {
            (PathBuf::from(snapshot), PathBuf::from(output), false)
        }
        (Some(snapshot), Some(output), Some("--check"), None) => {
            (PathBuf::from(snapshot), PathBuf::from(output), true)
        }
        _ => {
            return Err(
                "usage: stub-compiler <snapshot-directory> <output-blob-path> [--check]".into(),
            );
        }
    };

    let files = stub_files(&snapshot)
        .map_err(|error| format!("cannot walk {}: {error}", snapshot.display()))?;
    if files.is_empty() {
        // A wrong path must never silently produce an empty blob.
        return Err(format!("no stub files under {}", snapshot.display()).into());
    }

    let mut symbols = Vec::new();
    let mut warnings = 0usize;
    for path in &files {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("warning: skipping {}: {error}", path.display());
                warnings += 1;
                continue;
            }
        };
        let extraction = extract(&text);
        if extraction.had_parse_errors {
            eprintln!("warning: parse diagnostics in {}", path.display());
            warnings += 1;
        }
        symbols.extend(extraction.symbols);
    }

    let index = StubIndex::from_symbols(symbols);
    let blob = encode(&index);
    println!(
        "{} stub files, {} symbols, {} warnings, {} bytes",
        files.len(),
        index.len(),
        warnings,
        blob.len(),
    );

    if check {
        let committed = std::fs::read(&output)
            .map_err(|error| format!("cannot read {}: {error}", output.display()))?;
        if committed != blob {
            return Err(format!(
                "{} is stale: run `cargo xtask compile-stubs` and commit the result",
                output.display(),
            )
            .into());
        }
        println!("{} is up to date", output.display());
    } else {
        std::fs::write(&output, &blob)
            .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
        println!("wrote {}", output.display());
    }
    Ok(())
}
