//! The annotation ground-truth harness (design section 10, harness
//! 1): the hidden CLI channel run over the pinned corpus, gated
//! against a committed baseline classified by divergence class. The
//! gate is on regressions, never on the baseline's size - a drowning
//! protocol is no protocol.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineEntry {
    pub classification: String,
    pub symbol: String,
    pub record: String, // "<symbol>\t<inferred>\t<annotated>"
}

/// Skips `#` lines; each remaining line splits on the FIRST tab into
/// the classification and the record. A line without a tab is
/// ignored (a hand-editing accident must not poison the gate).
pub fn parse_baseline(text: &str) -> Vec<BaselineEntry> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let (classification, record) = line.split_once('\t')?;
            let symbol = record.split('\t').next().unwrap_or(record).to_owned();
            Some(BaselineEntry {
                classification: classification.to_owned(),
                symbol,
                record: record.to_owned(),
            })
        })
        .collect()
}

/// Produced records no baseline entry carries: the regressions the
/// gate fails on.
pub fn regressions(baseline: &[BaselineEntry], produced: &[String]) -> Vec<String> {
    let known: std::collections::BTreeSet<&str> =
        baseline.iter().map(|entry| entry.record.as_str()).collect();
    produced
        .iter()
        .filter(|record| !known.contains(record.as_str()))
        .cloned()
        .collect()
}

/// Baseline records the run no longer produces - printed as a
/// re-bless hint, never a failure.
pub fn stale(baseline: &[BaselineEntry], produced: &[String]) -> Vec<String> {
    let current: std::collections::BTreeSet<&str> = produced.iter().map(String::as_str).collect();
    baseline
        .iter()
        .filter(|entry| !current.contains(entry.record.as_str()))
        .map(|entry| entry.record.clone())
        .collect()
}

/// Per produced record: keep the existing classification when the
/// record persists; a new record auto-classifies `precision-gap`
/// when its inferred column is exactly `mixed`, else `unclassified`.
pub fn merge_baseline(existing: &[BaselineEntry], produced: &[String]) -> Vec<BaselineEntry> {
    let known: BTreeMap<&str, &str> = existing
        .iter()
        .map(|entry| (entry.record.as_str(), entry.classification.as_str()))
        .collect();
    produced
        .iter()
        .map(|record| {
            let mut columns = record.split('\t');
            let symbol = columns.next().unwrap_or("").to_owned();
            let inferred = columns.next().unwrap_or("");
            let classification = known
                .get(record.as_str())
                .copied()
                .unwrap_or(if inferred == "mixed" {
                    "precision-gap"
                } else {
                    "unclassified"
                })
                .to_owned();
            BaselineEntry {
                classification,
                symbol,
                record: record.clone(),
            }
        })
        .collect()
}

/// The committed baseline's path.
fn baseline_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("xtask/ground-truth-baseline.txt"))
}

/// Splits the trailing `checked N, divergences M` summary line from
/// the produced divergence records. The summary is recognized by its
/// `checked ` prefix, never taken on faith by position alone
/// (Finding 2, final review): a stream that stopped emitting a
/// summary must fail loudly here rather than have its last divergence
/// record silently swallowed and reported as merely stale.
fn split_summary(stdout: &str) -> Result<(&str, Vec<&str>)> {
    let mut lines: Vec<&str> = stdout.lines().collect();
    let summary = match lines.last() {
        Some(line) if line.starts_with("checked ") => lines.remove(lines.len() - 1),
        _ => return Err("the ground-truth stream printed no summary line".into()),
    };
    Ok((summary, lines))
}

/// Runs the built binary's hidden `ground-truth` channel over the
/// pinned corpus, splits its trailing `checked N, divergences M`
/// summary line from the produced divergence records, and either
/// gates the records against the committed baseline or (`bless`)
/// regenerates it, preserving every persisting record's hand-assigned
/// classification.
pub fn run(bless: bool) -> Result<()> {
    let corpus = crate::corpus::prepare()?;
    let binary = crate::release_binary()?;
    let output = Command::new(&binary)
        .arg("ground-truth")
        .arg(&corpus)
        .output()?;
    if output.status.code() != Some(0) {
        return Err(format!(
            "celerrate ground-truth did not complete (exit {:?}):\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
        )
        .into());
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("the ground-truth stream is not valid UTF-8: {error}"))?;

    let (summary, lines) = split_summary(&stdout)?;
    let produced: Vec<String> = lines.into_iter().map(str::to_owned).collect();
    println!("{summary}");

    if bless {
        bless_baseline(&produced)
    } else {
        gate(&produced)
    }
}

/// Gate mode: any produced record the baseline does not already carry
/// is a regression and fails the run; a baseline record the run no
/// longer produces is only a re-bless hint, never a failure. The
/// baseline's size never enters the decision.
fn gate(produced: &[String]) -> Result<()> {
    let path = baseline_path()?;
    let text = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read {}: {error}; run `cargo xtask ground-truth --bless` and triage the result",
            path.display(),
        )
    })?;
    let baseline = parse_baseline(&text);

    let stale_records = stale(&baseline, produced);
    if !stale_records.is_empty() {
        println!(
            "{} baseline record(s) no longer reproduce (consider `cargo xtask ground-truth --bless`):",
            stale_records.len(),
        );
        for record in &stale_records {
            println!("  {record}");
        }
    }

    let new_records = regressions(&baseline, produced);
    if !new_records.is_empty() {
        let actual_path = crate::workspace_root()?.join("target/corpus/actual-ground-truth.txt");
        std::fs::write(&actual_path, produced.join("\n"))?;
        return Err(format!(
            "{} divergence(s) absent from the committed baseline (regressions); the full \
             produced stream was written to {}:\n{}",
            new_records.len(),
            actual_path.display(),
            new_records.join("\n"),
        )
        .into());
    }

    println!("the ground-truth baseline holds");
    Ok(())
}

/// Bless mode: merges the produced records into the existing baseline
/// (preserving every persisting record's classification, per
/// `merge_baseline`) and writes the result back with its header.
fn bless_baseline(produced: &[String]) -> Result<()> {
    let path = baseline_path()?;
    let existing_text = std::fs::read_to_string(&path).unwrap_or_default();
    let existing = parse_baseline(&existing_text);
    let merged = merge_baseline(&existing, produced);

    let pin = crate::corpus::pin()?;
    let mut text = format!(
        "# celerrate ground-truth baseline — regenerate with: cargo xtask ground-truth --bless\n\
         # corpus: {} @ {}\n\
         # format: <classification>\\t<symbol>\\t<inferred>\\t<annotated>\n",
        pin.repository, pin.commit,
    );
    for entry in &merged {
        text.push_str(&entry.classification);
        text.push('\t');
        text.push_str(&entry.record);
        text.push('\n');
    }
    std::fs::write(&path, &text)?;
    println!("blessed {} ({} record(s))", path.display(), merged.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use super::{merge_baseline, parse_baseline, split_summary};

    #[test]
    fn the_summary_line_splits_from_the_produced_records() {
        let stdout = "app\\a\tmixed\tstring\ncheck ed\nchecked 18, divergences 2";
        let (summary, lines) = split_summary(stdout).unwrap();
        assert_eq!(summary, "checked 18, divergences 2");
        assert_eq!(lines, ["app\\a\tmixed\tstring", "check ed"]);
    }

    #[test]
    fn a_stream_missing_its_summary_line_fails_loudly() {
        // Finding 2 (final review): a stream that stopped emitting the
        // summary must never be mistaken for one whose last
        // divergence record simply looks like a summary — the last
        // line has to actually start with "checked " or the whole run
        // errors, rather than eating that record silently.
        let stdout = "app\\a\tmixed\tstring\napp\\b\tmixed\tint";
        assert!(split_summary(stdout).is_err());
    }

    #[test]
    fn an_empty_stream_fails_loudly() {
        assert!(split_summary("").is_err());
    }

    #[test]
    fn blessing_preserves_classifications_for_persisting_records() {
        let existing = parse_baseline("# header\nsuspected-inference-bug\tapp\\a\tmixed\tstring\n");
        let produced = vec![
            "app\\a\tmixed\tstring".to_owned(),
            "app\\b\tmixed\tint".to_owned(),
            "app\\c\t'x'\tint".to_owned(),
        ];
        let merged = merge_baseline(&existing, &produced);
        let classifications: Vec<(&str, &str)> = merged
            .iter()
            .map(|entry| (entry.classification.as_str(), entry.symbol.as_str()))
            .collect();
        assert_eq!(
            classifications,
            [
                ("suspected-inference-bug", "app\\a"),
                ("precision-gap", "app\\b"),
                ("unclassified", "app\\c"),
            ],
            "kept, auto-classified mixed, auto-classified other",
        );
    }

    #[test]
    fn the_gate_flags_only_records_absent_from_the_baseline() {
        let baseline = parse_baseline("precision-gap\tapp\\a\tmixed\tstring\n");
        let produced = vec![
            "app\\a\tmixed\tstring".to_owned(),
            "app\\new\tmixed\tint".to_owned(),
        ];
        let new_records = super::regressions(&baseline, &produced);
        assert_eq!(new_records, ["app\\new\tmixed\tint"]);
    }

    #[test]
    fn a_stale_baseline_entry_is_reported_but_never_fails_the_gate() {
        let baseline = parse_baseline("precision-gap\tapp\\gone\tmixed\tstring\n");
        let produced: Vec<String> = Vec::new();
        assert!(super::regressions(&baseline, &produced).is_empty());
        assert_eq!(
            super::stale(&baseline, &produced),
            ["app\\gone\tmixed\tstring"]
        );
    }
}
