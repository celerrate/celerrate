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

use celerrate_types::TypedFileResult;

/// One session's cache traffic. Atomic because the item-tree lookups
/// happen under the rayon fan-out. The counters are never reset between
/// watch cycles: they accumulate for the whole session's lifetime, so
/// under `--watch` each per-cycle stderr line reports running totals
/// across cycles, not a per-cycle delta.
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
    /// Bodies the typed families walked this run. Never cached: plan
    /// 5's decision 13 keeps these counters at the orchestration layer,
    /// aggregated once per file regardless of cache hit or miss.
    pub typed_bodies: AtomicU64,
    /// Interprocedural edges the walked bodies consumed, declared tier.
    pub typed_declared_edges: AtomicU64,
    /// Interprocedural edges the walked bodies consumed, inferred tier.
    pub typed_inferred_edges: AtomicU64,
    /// Interprocedural edges the walked bodies consumed, provider tier.
    pub typed_provider_edges: AtomicU64,
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
            "cache: trees {} hit / {} miss; verdicts {} served / {} discarded / {} absent; typed {} bodies, edges {} declared / {} inferred / {} provider; persist {} written / {} skipped / {} failed",
            load(&self.tree_hits),
            load(&self.tree_misses),
            load(&self.verdicts_served),
            load(&self.verdicts_discarded),
            load(&self.verdicts_absent),
            load(&self.typed_bodies),
            load(&self.typed_declared_edges),
            load(&self.typed_inferred_edges),
            load(&self.typed_provider_edges),
            load(&self.persist_written),
            load(&self.persist_skipped),
            load(&self.persist_failed),
        )
    }

    /// Aggregates one file's typed instrument (plan 5's decision 13:
    /// counters live at the orchestration layer, never inside queries).
    pub fn record_typed(&self, result: &TypedFileResult) {
        self.typed_bodies
            .fetch_add(u64::from(result.bodies), Ordering::Relaxed);
        self.typed_declared_edges.fetch_add(
            u64::from(result.edge_counts.declared_return_edges),
            Ordering::Relaxed,
        );
        self.typed_inferred_edges.fetch_add(
            u64::from(result.edge_counts.inferred_return_edges),
            Ordering::Relaxed,
        );
        self.typed_provider_edges.fetch_add(
            u64::from(result.edge_counts.provider_edges),
            Ordering::Relaxed,
        );
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

    use celerrate_types::{InterproceduralEdgeCounts, TypedFileResult};

    use super::{CacheStatistics, wants_statistics};

    #[test]
    fn record_typed_aggregates_across_files() {
        let statistics = CacheStatistics::default();
        let result = TypedFileResult {
            bodies: 2,
            edge_counts: InterproceduralEdgeCounts {
                declared_return_edges: 3,
                inferred_return_edges: 1,
                provider_edges: 5,
            },
            ..Default::default()
        };
        statistics.record_typed(&result);
        statistics.record_typed(&result);
        assert_eq!(statistics.typed_bodies.load(Ordering::Relaxed), 4);
        assert_eq!(statistics.typed_declared_edges.load(Ordering::Relaxed), 6);
        assert_eq!(statistics.typed_inferred_edges.load(Ordering::Relaxed), 2);
        assert_eq!(statistics.typed_provider_edges.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn the_rendered_line_carries_every_counter() {
        let statistics = CacheStatistics::default();
        statistics.tree_hits.fetch_add(3, Ordering::Relaxed);
        statistics.verdicts_served.fetch_add(2, Ordering::Relaxed);
        statistics.typed_bodies.fetch_add(4, Ordering::Relaxed);
        statistics
            .typed_declared_edges
            .fetch_add(5, Ordering::Relaxed);
        statistics
            .typed_inferred_edges
            .fetch_add(6, Ordering::Relaxed);
        statistics
            .typed_provider_edges
            .fetch_add(7, Ordering::Relaxed);
        statistics.persist_failed.fetch_add(1, Ordering::Relaxed);
        assert_eq!(
            statistics.render(),
            "cache: trees 3 hit / 0 miss; verdicts 2 served / 0 discarded / 0 absent; typed 4 bodies, edges 5 declared / 6 inferred / 7 provider; persist 0 written / 0 skipped / 1 failed",
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
