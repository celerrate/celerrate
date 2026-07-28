//! The baseline: known findings hidden from the report and the exit code.
//!
//! Entirely CLI-layer machinery. Nothing here enters a salsa query or the
//! persistent cache: the baseline is presentation, applied after analysis
//! and suppression, before rendering and the exit code.

pub mod entry;
pub mod file;
pub mod symbol;

use std::collections::BTreeMap;
use std::io;

use celerrate_diagnostics::Diagnostic;

use crate::session::Session;
use entry::{BaselineEntry, BaselineKey};

/// The fixed file name at the project root. No configuration key moves it.
pub const BASELINE_FILE_NAME: &str = "celerrate-baseline.toml";

/// How this run treats the baseline file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A present file applies automatically (the default).
    Apply,
    /// `--baseline`: record or rewrite the file from the current findings.
    Record,
    /// `--ignore-baseline`: strict run, the file is not consulted.
    Ignore,
}

impl Mode {
    pub fn of(record: bool, ignore: bool) -> Self {
        // clap guarantees record and ignore are mutually exclusive.
        if record {
            Self::Record
        } else if ignore {
            Self::Ignore
        } else {
            Self::Apply
        }
    }
}

/// What the baseline step did this run; consumed by rendering and the
/// exit-code computation.
#[derive(Debug, Default)]
pub struct BaselineOutcome {
    /// Diagnostics removed from the report and the exit code.
    pub hidden: usize,
    /// Entry count written by `--baseline`, when recording.
    pub recorded: Option<usize>,
}

/// The structural key of one span-anchored diagnostic; `None` for
/// project-anchored findings (the baseline covers spans only).
pub fn fingerprint(session: &Session, diagnostic: &Diagnostic) -> Option<BaselineKey> {
    let (file, range) = diagnostic.span()?;
    let absolute = session.vfs.path(file)?;
    let relative = absolute
        .strip_prefix(&session.discovery.root)
        .unwrap_or(absolute);
    let path = relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    let symbol = symbol::enclosing_symbol_path(&session.database, session.files, file, range);
    Some(BaselineKey {
        path,
        identifier: diagnostic.id.as_str().to_string(),
        symbol,
        message: diagnostic.message.clone(),
    })
}

/// Records the given diagnostics into `celerrate-baseline.toml` at the
/// project root. Returns `Ok(None)` when the file was left genuinely
/// untouched (no entries and no existing file to rewrite), and
/// `Ok(Some(entry count))` whenever a write actually occurred: the caller's
/// report must never claim a recording that did not happen. Never deletes
/// the file: a now-clean project rewrites it header-only when it exists.
pub fn record(session: &Session, diagnostics: &[Diagnostic]) -> io::Result<Option<usize>> {
    let mut counts: BTreeMap<BaselineKey, u32> = BTreeMap::new();
    for diagnostic in diagnostics {
        if let Some(key) = fingerprint(session, diagnostic) {
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let entries: Vec<BaselineEntry> = counts
        .into_iter()
        .map(|(key, count)| BaselineEntry {
            path: key.path,
            identifier: key.identifier,
            symbol: key.symbol,
            message: key.message,
            count,
        })
        .collect();
    let path = session.discovery.root.join(BASELINE_FILE_NAME);
    if entries.is_empty() && !path.exists() {
        return Ok(None);
    }
    crate::cache::pack::write_atomically(&path, file::serialize(&entries).as_bytes())?;
    Ok(Some(entries.len()))
}
