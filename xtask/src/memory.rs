//! The peak-memory measurement behind the type engine's closure
//! criterion: the pinned corpus analyzed cold, then warm, with
//! everything the shipped binary enables, peak RSS parsed from
//! `/usr/bin/time`, the cold number gated against the budget. External
//! measurement for the same reason the protocol uses hyperfine: the
//! number includes everything the process allocates, not what an
//! in-process probe remembers to count.

use std::path::Path;
use std::process::Command;

use crate::Result;

/// The cold peak-RSS budget on the corpus, in bytes: 1.5 GiB,
/// reconducted from the semantic core's closure budget. The warm
/// number is recorded, never gated: a warm run reuses the cache and
/// sits below the cold peak or the cache is doing something wrong that
/// other gates catch.
pub const PEAK_MEMORY_CEILING_BYTES: u64 = 1_610_612_736;

const MEBIBYTE: u64 = 1024 * 1024;

/// Measures the cold and warm peak RSS on the pinned corpus and prints
/// both. With `check_ceiling`, a cold peak over the budget fails the
/// run.
pub fn run(check_ceiling: bool) -> Result<()> {
    let corpus = crate::corpus::prepare()?;
    let binary = crate::release_binary()?;
    let cache = corpus.join(".celerrate");
    if cache.exists() {
        std::fs::remove_dir_all(&cache)?;
    }
    let cold = measure(&binary, &corpus)?;
    let warm = measure(&binary, &corpus)?;
    println!("{:<16} {:>12}", "run", "peak rss");
    println!("{:<16} {:>8} MiB", "cold full", cold / MEBIBYTE);
    println!("{:<16} {:>8} MiB", "warm no-change", warm / MEBIBYTE);
    if check_ceiling && let Some(failure) = over_budget(cold) {
        return Err(failure.into());
    }
    Ok(())
}

/// One analysis under `/usr/bin/time`, peak RSS parsed from its
/// stderr. Exit 1 means diagnostics were reported — a completed
/// analysis, exactly as the benchmark and priming runs treat it.
fn measure(binary: &Path, working: &Path) -> Result<u64> {
    let flag = if cfg!(target_os = "macos") {
        "-l"
    } else {
        "-v"
    };
    let output = Command::new("/usr/bin/time")
        .arg(flag)
        .arg(binary)
        .args(["check", "."])
        .current_dir(working)
        .stdout(std::process::Stdio::null())
        .output()?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "the measured run did not complete (exit {:?})",
            output.status.code()
        )
        .into());
    }
    peak_bytes(&String::from_utf8_lossy(&output.stderr))
}

/// Parses the peak resident set size, in bytes, from `/usr/bin/time`
/// output: the BSD `-l` dialect (a byte count on a line ending in
/// `maximum resident set size`) and the GNU `-v` dialect
/// (`Maximum resident set size (kbytes): N`).
pub fn peak_bytes(output: &str) -> Result<u64> {
    for line in output.lines() {
        if let Some((_, value)) = line.split_once("Maximum resident set size (kbytes):") {
            let kibibytes: u64 = value
                .trim()
                .parse()
                .map_err(|error| format!("unreadable peak RSS {value:?}: {error}"))?;
            return Ok(kibibytes.saturating_mul(1024));
        }
        if let Some(value) = line.trim().strip_suffix("maximum resident set size") {
            return value
                .trim()
                .parse()
                .map_err(|error| format!("unreadable peak RSS {value:?}: {error}").into());
        }
    }
    Err("the time output carries no maximum resident set size".into())
}

/// The budget comparison, named like `bench::over_ceiling`.
pub fn over_budget(cold_peak_bytes: u64) -> Option<String> {
    (cold_peak_bytes > PEAK_MEMORY_CEILING_BYTES).then(|| {
        format!(
            "the cold peak RSS ({} MiB) is over the {} MiB budget",
            cold_peak_bytes / MEBIBYTE,
            PEAK_MEMORY_CEILING_BYTES / MEBIBYTE
        )
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{PEAK_MEMORY_CEILING_BYTES, over_budget, peak_bytes};

    #[test]
    fn the_macos_time_output_reports_bytes() {
        let output = "        1.23 real         4.56 user         0.78 sys\n           123456789  maximum resident set size\n              1111  peak memory footprint\n";
        assert_eq!(peak_bytes(output).unwrap(), 123_456_789);
    }

    #[test]
    fn the_gnu_time_output_reports_kibibytes() {
        let output = "\tCommand being timed: \"celerrate check .\"\n\tMaximum resident set size (kbytes): 524288\n\tExit status: 0\n";
        assert_eq!(peak_bytes(output).unwrap(), 524_288 * 1024);
    }

    #[test]
    fn time_output_without_a_peak_is_an_error_not_a_panic() {
        assert!(peak_bytes("").is_err());
        assert!(peak_bytes("        1.23 real  0.1 user  0.1 sys").is_err());
        assert!(peak_bytes("Maximum resident set size (kbytes): not-a-number").is_err());
    }

    #[test]
    fn a_cold_peak_over_the_budget_is_named() {
        let failure = over_budget(PEAK_MEMORY_CEILING_BYTES + 1).unwrap();
        assert!(failure.contains("1536 MiB"));
        assert!(over_budget(PEAK_MEMORY_CEILING_BYTES).is_none());
    }
}
