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

use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};

use crate::session::Session;
use entry::{BaselineEntry, BaselineKey};

/// The fixed file name at the project root. No configuration key moves it.
pub const BASELINE_FILE_NAME: &str = "celerrate-baseline.toml";

/// Obsolete baseline entries: recorded findings that no longer match.
pub const OBSOLETE_BASELINE_ENTRIES: DiagnosticId = DiagnosticId::new("CEL0050");
/// The baseline file exists but could not be (fully) read.
pub const INVALID_BASELINE_FILE: DiagnosticId = DiagnosticId::new("CEL0051");
/// Checked against the registry by the composition-root guard (task 8).
pub const ALLOCATED_IDENTIFIERS: &[DiagnosticId] =
    &[OBSOLETE_BASELINE_ENTRIES, INVALID_BASELINE_FILE];

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
    /// Notices about the baseline file itself: exit-neutral by
    /// construction, rendered like `ProjectNotice`, never entering
    /// `AnalysisOutcome::diagnostics`.
    pub notices: Vec<BaselineNotice>,
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

/// The baseline file as loaded at session start: parsed entries plus the
/// failure lines of whatever did not parse.
#[derive(Debug)]
pub struct LoadedBaseline {
    pub entries: Vec<BaselineEntry>,
    pub failures: Vec<String>,
}

/// Reads `<root>/celerrate-baseline.toml`. `None` when absent; a present
/// but unreadable file yields no entries and a failure line. This
/// follows the resilience rule: never crash, and never hide silently.
pub fn load(root: &std::path::Path) -> Option<LoadedBaseline> {
    let path = root.join(BASELINE_FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(LoadedBaseline {
                entries: Vec::new(),
                failures: vec![format!("the file could not be read: {error}")],
            });
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            return Some(LoadedBaseline {
                entries: Vec::new(),
                failures: vec!["the file is not valid UTF-8".to_string()],
            });
        }
    };
    let parsed = file::parse(&text);
    Some(LoadedBaseline {
        entries: parsed.entries,
        failures: parsed.failures,
    })
}

/// An exit-neutral, project-anchored baseline notice. Rides the notice
/// channel (like `ProjectNotice`), never `AnalysisOutcome::diagnostics`,
/// so it cannot reach the exit code by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineNotice {
    InvalidFile { detail: String },
    ObsoleteEntries { count: usize },
}

impl BaselineNotice {
    pub fn identifier(&self) -> DiagnosticId {
        match self {
            Self::InvalidFile { .. } => INVALID_BASELINE_FILE,
            Self::ObsoleteEntries { .. } => OBSOLETE_BASELINE_ENTRIES,
        }
    }

    pub fn severity(&self) -> Severity {
        Severity::Warning
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidFile { detail } => format!(
                "{BASELINE_FILE_NAME} could not be fully read ({detail}); unreadable entries are ignored and their findings reported"
            ),
            Self::ObsoleteEntries { count: 1 } => "1 baseline entry no longer matches the current findings; re-record with `celerrate check --baseline`".to_string(),
            Self::ObsoleteEntries { count } => format!(
                "{count} baseline entries no longer match the current findings; re-record with `celerrate check --baseline`"
            ),
        }
    }
}

/// Applies the session's loaded baseline to the diagnostic list, in place.
/// Matching consumes at most `count` occurrences per key; occurrence
/// `count + 1` stays reported. An entry that consumed fewer occurrences
/// than its count is obsolete -- surplus capacity that could silently
/// absorb a future regression -- and is reported through one aggregated
/// [`BaselineNotice::ObsoleteEntries`], never per entry.
pub fn apply(session: &Session, diagnostics: &mut Vec<Diagnostic>) -> BaselineOutcome {
    let Some(loaded) = session.loaded_baseline.as_ref() else {
        return BaselineOutcome::default();
    };
    let mut notices = Vec::new();
    if !loaded.failures.is_empty() {
        notices.push(BaselineNotice::InvalidFile {
            detail: loaded.failures.join("; "),
        });
    }
    let mut remaining: BTreeMap<BaselineKey, u32> = BTreeMap::new();
    for entry in &loaded.entries {
        *remaining.entry(entry.key()).or_insert(0) += entry.count;
    }
    let before = diagnostics.len();
    diagnostics.retain(|diagnostic| {
        let Some(key) = fingerprint(session, diagnostic) else {
            return true;
        };
        match remaining.get_mut(&key) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        }
    });
    let obsolete = remaining.values().filter(|capacity| **capacity > 0).count();
    if obsolete > 0 {
        notices.push(BaselineNotice::ObsoleteEntries { count: obsolete });
    }
    BaselineOutcome {
        hidden: before - diagnostics.len(),
        recorded: None,
        notices,
    }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn the_allocation_list_is_exactly_what_the_notices_use() {
        let used = [
            BaselineNotice::ObsoleteEntries { count: 1 }.identifier(),
            BaselineNotice::InvalidFile {
                detail: String::new(),
            }
            .identifier(),
        ];
        assert_eq!(ALLOCATED_IDENTIFIERS, used.as_slice());
    }
}
