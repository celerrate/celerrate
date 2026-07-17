//! The stub compiler: pinned snapshot in, committed blob out. Driven
//! by `cargo xtask compile-stubs`, never by a build script. Malformed
//! stub files produce warnings and partial extraction; only a missing
//! snapshot or an unwritable output fails the run.

use std::path::PathBuf;
use std::process::ExitCode;

use celerrate_stubs::compiler::extract::extract;
use celerrate_stubs::compiler::refinement_source::{parse_refinement_source, validate_refinements};
use celerrate_stubs::compiler::snapshot::stub_files;
use celerrate_stubs::{StubIndex, encode};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const USAGE: &str =
    "usage: stub-compiler <snapshot-directory> <output-blob-path> [--refinements <path>] [--check]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Parsed command-line arguments: the two required positionals plus
/// the optional `--refinements <path>` and `--check` flags, accepted
/// in either order.
struct Arguments {
    snapshot: PathBuf,
    output: PathBuf,
    refinements: Option<PathBuf>,
    check: bool,
}

fn parse_arguments() -> Result<Arguments> {
    let mut positionals = Vec::new();
    let mut refinements = None;
    let mut check = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--check" => check = true,
            "--refinements" => {
                let path = arguments.next().ok_or("--refinements needs a path")?;
                refinements = Some(PathBuf::from(path));
            }
            _ => positionals.push(argument),
        }
    }
    let [snapshot, output] = <[String; 2]>::try_from(positionals).map_err(|_| USAGE)?;
    Ok(Arguments {
        snapshot: PathBuf::from(snapshot),
        output: PathBuf::from(output),
        refinements,
        check,
    })
}

fn run() -> Result<()> {
    let Arguments {
        snapshot,
        output,
        refinements,
        check,
    } = parse_arguments()?;

    let files = stub_files(&snapshot)
        .map_err(|error| format!("cannot walk {}: {error}", snapshot.display()))?;
    if files.is_empty() {
        // A wrong path must never silently produce an empty blob.
        return Err(format!("no stub files under {}", snapshot.display()).into());
    }

    let mut symbols = Vec::new();
    let mut functions = Vec::new();
    let mut classes = Vec::new();
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
        functions.extend(extraction.functions);
        classes.extend(extraction.classes);
    }

    let mut index = StubIndex::new(symbols, functions, classes);

    if let Some(refinements_path) = &refinements {
        let text = std::fs::read_to_string(refinements_path)
            .map_err(|error| format!("cannot read {}: {error}", refinements_path.display()))?;
        let parsed = parse_refinement_source(&text)
            .map_err(|error| format!("{}: {error}", refinements_path.display()))?;
        validate_refinements(&parsed, index.functions(), index.classes())
            .map_err(|error| format!("{}: {error}", refinements_path.display()))?;
        index.set_refinements(parsed);
    }

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
