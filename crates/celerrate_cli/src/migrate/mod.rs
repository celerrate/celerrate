//! `celerrate migrate --from-phpstan`: convert a PHPStan project to
//! Celerrate in one command. Parse `phpstan.neon` (a minimal NEON
//! subset), generate `celerrate.toml`, report what does not carry
//! over, and record the baseline so only new problems fail.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Outcome;
use crate::session::Session;

pub(crate) mod convert;
pub(crate) mod neon;
pub(crate) mod settings;

/// The name PHPStan documents first, and the one the report names when
/// a discovered path somehow has no final component.
const PRIMARY_SOURCE_FILE_NAME: &str = "phpstan.neon";

/// PHPStan's own configuration discovery order.
pub(crate) const SOURCE_FILE_NAMES: [&str; 3] = [
    PRIMARY_SOURCE_FILE_NAME,
    "phpstan.neon.dist",
    "phpstan.dist.neon",
];

const TARGET_FILE_NAME: &str = "celerrate.toml";

/// The whole command: discover, convert, write, report, then record
/// the clean slate. The root has already been validated and
/// absolutized by the dispatcher.
pub(crate) fn execute(root: &Path, force: bool, output: &mut dyn Write) -> Outcome {
    let Some(source) = discover(root) else {
        let _ = writeln!(
            output,
            "error: no PHPStan configuration found in {}: expected one of {}",
            root.display(),
            SOURCE_FILE_NAMES.join(", "),
        );
        return Outcome::UsageError;
    };
    let target = root.join(TARGET_FILE_NAME);
    if target.exists() && !force {
        let _ = writeln!(
            output,
            "error: {TARGET_FILE_NAME} already exists; pass --force to overwrite it",
        );
        return Outcome::UsageError;
    }
    let settings = match settings::load(&source, root) {
        Ok(settings) => settings,
        Err(message) => {
            let _ = writeln!(output, "error: {message}");
            return Outcome::InternalError;
        }
    };
    let conversion = convert::convert(&settings);
    if let Err(error) = crate::cache::pack::write_atomically(&target, conversion.toml.as_bytes()) {
        let _ = writeln!(output, "error: could not write {TARGET_FILE_NAME}: {error}");
        return Outcome::InternalError;
    }
    let source_name = source_file_name(&source);
    let _ = render_report(output, &source_name, &conversion, &settings);
    record_clean_slate(root, output)
}

fn discover(root: &Path) -> Option<PathBuf> {
    SOURCE_FILE_NAMES
        .iter()
        .map(|name| root.join(name))
        .find(|candidate| candidate.is_file())
}

/// The discovered source's own file name, for the report. `discover`
/// always joins one of [`SOURCE_FILE_NAMES`], so the fallback is
/// unreachable; it exists because saying so would need a panic.
fn source_file_name(source: &Path) -> String {
    source.file_name().map_or_else(
        || PRIMARY_SOURCE_FILE_NAME.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn render_report(
    output: &mut dyn Write,
    source: &str,
    conversion: &convert::Conversion,
    settings: &settings::Settings,
) -> std::io::Result<()> {
    writeln!(output, "migrated {source} to {TARGET_FILE_NAME}")?;
    writeln!(
        output,
        "  include: {}, exclude: {}",
        crate::render::count(conversion.include.len(), "path", "paths"),
        crate::render::count(conversion.exclude.len(), "path", "paths"),
    )?;
    writeln!(output, "  {}", conversion.level_note)?;
    for (path, reason) in &conversion.dropped {
        writeln!(output, "  dropped {path}: {reason}")?;
    }
    if !settings.untransposed.is_empty() {
        writeln!(output, "not carried over:")?;
        for entry in &settings.untransposed {
            writeln!(
                output,
                "  {} ({}): {}",
                entry.key,
                entry.origin,
                explanation(&entry.key),
            )?;
        }
    }
    if !settings.problems.is_empty() {
        writeln!(output, "not parsed:")?;
        for problem in &settings.problems {
            writeln!(output, "  {problem}")?;
        }
    }
    if !settings.ignored_includes.is_empty() {
        writeln!(
            output,
            "ignored includes (never parsed; delete them once the baseline is in place):",
        )?;
        for include in &settings.ignored_includes {
            writeln!(output, "  {include}")?;
        }
    }
    Ok(())
}

/// One honest line per key the migration does not carry over. The
/// report is the migration documentation: generated, never silent.
fn explanation(key: &str) -> &'static str {
    match key {
        "ignoreErrors" => {
            "message patterns over PHPStan's vocabulary do not translate; the recorded baseline carries the continuity"
        }
        "bootstrap" | "bootstrapFiles" => "Celerrate does not execute project code before analysis",
        "stubFiles" => "PHPStan stub files are not consumed; Celerrate ships its own stubs",
        "scanFiles" | "scanDirectories" => "symbol discovery follows Composer autoloading",
        "phpVersion" => {
            "set `php` under `[project]` in celerrate.toml if the Composer range is not right"
        }
        "tmpDir" | "parallel" => "Celerrate manages its own cache and parallelism",
        "services" | "rules" | "conditionalTags" => {
            "PHPStan extensions have no Celerrate equivalent; first-party plugins are enabled by default"
        }
        _ => "no Celerrate equivalent in v0.1",
    }
}

/// The clean slate: always run the analysis, record the baseline when
/// there are findings, no file when there are none. Mirrors the check
/// pipeline exactly (suppression is in-engine, upstream; the
/// configuration diagnostics of the generated file are never merged
/// into the recorded slice, so they cannot be baselined).
fn record_clean_slate(root: &Path, output: &mut dyn Write) -> Outcome {
    let mut session = Session::start(root);
    let inputs = session.inputs();
    let outcome = crate::single_pass(&mut session, || crate::analysis::analyze(&inputs));
    session.absorb_outcome(&outcome);
    let diagnostics = crate::suggest::enrich(&session, &outcome.diagnostics);
    match crate::baseline::record(&session, &diagnostics) {
        Ok(Some(recorded)) => {
            let _ = writeln!(
                output,
                "recorded {} to {}",
                crate::render::count(recorded, "baseline entry", "baseline entries"),
                crate::baseline::BASELINE_FILE_NAME,
            );
        }
        Ok(None) => {
            let _ = writeln!(output, "no findings: no baseline needed");
        }
        Err(error) => {
            let _ = writeln!(
                output,
                "error: could not write {}: {error}",
                crate::baseline::BASELINE_FILE_NAME,
            );
            return Outcome::InternalError;
        }
    }
    crate::cache::persist(&mut session, &outcome);
    if crate::configuration::diagnostic_count(&session) > 0 {
        let _ = writeln!(
            output,
            "warning: the generated {TARGET_FILE_NAME} produced configuration diagnostics; run `celerrate check`",
        );
    }
    if !session.internal_errors.is_empty() {
        let _ = crate::render::render_internal_errors(output, &session);
        return Outcome::InternalError;
    }
    let _ = writeln!(
        output,
        "run `celerrate check`: from here, only new problems fail",
    );
    Outcome::Clean
}
