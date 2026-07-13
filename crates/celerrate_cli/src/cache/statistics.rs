//! Cache observability (audit findings I8 and M5): cheap process-wide
//! counters, printed to stderr when `CELERRATE_CACHE_STATS=1`. The
//! counters never feed analysis — salsa's determinism is untouched —
//! and the stderr line is not a contractual surface; it exists so the
//! parent spec's economics rule ("an artifact class that does not pay
//! for itself is dropped") is measurable without a profiler: hit rate,
//! revalidation acceptance, and persist health, per class. The
//! environment variable is read here, at the orchestration layer,
//! never inside a query.

use std::sync::atomic::{AtomicU64, Ordering};

/// One session's cache traffic. Atomic because the item-tree lookups
/// happen under the rayon fan-out.
#[derive(Debug, Default)]
pub struct CacheStatistics {
    /// Item-tree lookups answered from the pack.
    pub tree_hits: AtomicU64,
    /// Item-tree lookups the pack could not answer.
    pub tree_misses: AtomicU64,
    /// Verdicts served: present, every record revalidated, every
    /// diagnostic converted.
    pub verdicts_served: AtomicU64,
    /// Verdicts present but refused: a record's answer moved, or a
    /// stored diagnostic failed conversion.
    pub verdicts_discarded: AtomicU64,
    /// Verdicts absent: no entry under the file's content hash.
    pub verdicts_absent: AtomicU64,
    /// Pack writes that happened.
    pub persist_written: AtomicU64,
    /// Pack writes skipped because nothing changed.
    pub persist_skipped: AtomicU64,
    /// Pack writes that failed — the silent failure of audit finding
    /// M5, now at least countable.
    pub persist_failed: AtomicU64,
}

impl CacheStatistics {
    /// The one-line summary the environment variable asks for.
    pub fn render(&self) -> String {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        format!(
            "cache: trees {} hit / {} miss; verdicts {} served / {} discarded / {} absent; persist {} written / {} skipped / {} failed",
            load(&self.tree_hits),
            load(&self.tree_misses),
            load(&self.verdicts_served),
            load(&self.verdicts_discarded),
            load(&self.verdicts_absent),
            load(&self.persist_written),
            load(&self.persist_skipped),
            load(&self.persist_failed),
        )
    }

    /// Prints the line to stderr when `CELERRATE_CACHE_STATS=1`.
    pub fn report(&self) {
        let variable = std::env::var("CELERRATE_CACHE_STATS").ok();
        if wants_statistics(variable.as_deref()) {
            eprintln!("{}", self.render());
        }
    }
}

/// The gate, as a pure function so it is testable without mutating the
/// process environment.
fn wants_statistics(value: Option<&str>) -> bool {
    value == Some("1")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::{CacheStatistics, wants_statistics};

    #[test]
    fn the_rendered_line_carries_every_counter() {
        let statistics = CacheStatistics::default();
        statistics.tree_hits.fetch_add(3, Ordering::Relaxed);
        statistics.verdicts_served.fetch_add(2, Ordering::Relaxed);
        statistics.persist_failed.fetch_add(1, Ordering::Relaxed);
        assert_eq!(
            statistics.render(),
            "cache: trees 3 hit / 0 miss; verdicts 2 served / 0 discarded / 0 absent; persist 0 written / 0 skipped / 1 failed",
        );
    }

    #[test]
    fn only_the_exact_opt_in_enables_the_report() {
        assert!(wants_statistics(Some("1")));
        assert!(!wants_statistics(Some("0")));
        assert!(!wants_statistics(Some("true")));
        assert!(!wants_statistics(None));
    }
}
