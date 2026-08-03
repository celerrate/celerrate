//! Per-phase wall-clock timings for one `check` pass, printed on the
//! `--verbose` channel. Meta-reporting only, like `verbose.rs`: the
//! machine formats stay byte-identical with or without the flag,
//! nothing here enters a salsa query, and the line format is not a
//! stable surface. Wall-clock reads happen at the recording call
//! sites, which are all orchestration code — the same legality
//! argument as `cache::persist`'s own timer. Under `--watch` the
//! counters accumulate across cycles rather than resetting; the
//! channel reports totals for the session, which is what profiling a
//! long-running watch wants anyway.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The measured phases of one `check` pass, in pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Walk,
    ReadAndSetInputs,
    Analysis,
    Enrich,
    Render,
    PersistCollectEntries,
    PersistCollectSignatures,
    PersistPackWrites,
}

impl Phase {
    /// Every phase, in the order the lines print.
    const ALL: [Phase; 8] = [
        Phase::Walk,
        Phase::ReadAndSetInputs,
        Phase::Analysis,
        Phase::Enrich,
        Phase::Render,
        Phase::PersistCollectEntries,
        Phase::PersistCollectSignatures,
        Phase::PersistPackWrites,
    ];

    fn label(self) -> &'static str {
        match self {
            Phase::Walk => "filesystem walk",
            Phase::ReadAndSetInputs => "file read + input set",
            Phase::Analysis => "analysis fan-out",
            Phase::Enrich => "suggest enrich",
            Phase::Render => "render report",
            Phase::PersistCollectEntries => "persist: collect entries",
            Phase::PersistCollectSignatures => "persist: collect signatures",
            Phase::PersistPackWrites => "persist: pack writes",
        }
    }
}

/// Accumulated milliseconds per phase. Atomics for the same reason as
/// `CacheStatistics`: the value is shared through `Arc` with call
/// sites that hold `&Session`, and a relaxed counter is all a
/// telemetry total needs.
#[derive(Debug, Default)]
pub struct PhaseTimings {
    walk: AtomicU64,
    read_and_set_inputs: AtomicU64,
    analysis: AtomicU64,
    enrich: AtomicU64,
    render: AtomicU64,
    persist_collect_entries: AtomicU64,
    persist_collect_signatures: AtomicU64,
    persist_pack_writes: AtomicU64,
}

impl PhaseTimings {
    /// The counter behind one phase. A match, not an index: the
    /// workspace denies `indexing_slicing`, and the match is exhaustive
    /// by construction.
    fn counter(&self, phase: Phase) -> &AtomicU64 {
        match phase {
            Phase::Walk => &self.walk,
            Phase::ReadAndSetInputs => &self.read_and_set_inputs,
            Phase::Analysis => &self.analysis,
            Phase::Enrich => &self.enrich,
            Phase::Render => &self.render,
            Phase::PersistCollectEntries => &self.persist_collect_entries,
            Phase::PersistCollectSignatures => &self.persist_collect_signatures,
            Phase::PersistPackWrites => &self.persist_pack_writes,
        }
    }

    /// Adds an elapsed duration to a phase's total, saturating rather
    /// than panicking on the absurd overflow case.
    pub fn record(&self, phase: Phase, elapsed: Duration) {
        let milliseconds = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        self.counter(phase)
            .fetch_add(milliseconds, Ordering::Relaxed);
    }

    /// One line per phase, in pipeline order, zeros included: a phase
    /// that never ran (a machine-format pass skips rich rendering)
    /// still prints, so the reader sees the whole pipeline shape.
    pub fn render_lines(&self) -> Vec<String> {
        Phase::ALL
            .iter()
            .map(|&phase| {
                format!(
                    "verbose: phase {}: {}ms",
                    phase.label(),
                    self.counter(phase).load(Ordering::Relaxed),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use std::time::Duration;

    use super::{Phase, PhaseTimings};

    #[test]
    fn every_phase_renders_one_line_in_pipeline_order() {
        let timings = PhaseTimings::default();
        let lines = timings.render_lines();
        assert_eq!(
            lines,
            vec![
                "verbose: phase filesystem walk: 0ms",
                "verbose: phase file read + input set: 0ms",
                "verbose: phase analysis fan-out: 0ms",
                "verbose: phase suggest enrich: 0ms",
                "verbose: phase render report: 0ms",
                "verbose: phase persist: collect entries: 0ms",
                "verbose: phase persist: collect signatures: 0ms",
                "verbose: phase persist: pack writes: 0ms",
            ],
        );
    }

    #[test]
    fn recording_adds_milliseconds_to_the_named_phase_only() {
        let timings = PhaseTimings::default();
        timings.record(Phase::Enrich, Duration::from_millis(120));
        timings.record(Phase::Enrich, Duration::from_millis(30));
        let lines = timings.render_lines();
        assert_eq!(lines[3], "verbose: phase suggest enrich: 150ms");
        assert_eq!(lines[0], "verbose: phase filesystem walk: 0ms");
    }

    #[test]
    fn a_sub_millisecond_duration_still_renders_as_zero() {
        let timings = PhaseTimings::default();
        timings.record(Phase::Walk, Duration::from_micros(400));
        assert_eq!(
            timings.render_lines()[0],
            "verbose: phase filesystem walk: 0ms",
        );
    }
}
