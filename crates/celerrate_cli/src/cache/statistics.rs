//! Cache observability (audit findings I8 and M5): cheap process-wide
//! counters, printed to stderr when `CELERRATE_CACHE_STATS=1`. The
//! counters never feed analysis — salsa's determinism is untouched —
//! and the stderr line is not a contractual surface; it exists so the
//! parent spec's economics rule ("an artifact class that does not pay
//! for itself is dropped") is measurable without a profiler: hit rate,
//! revalidation acceptance, and persist health, per class. The
//! environment variable is read here, at the orchestration layer,
//! never inside a query.
//!
//! Recorded ledger note (task 12): whether an in-memory LRU capacity
//! belongs on top of these counters remains plan 9b's decision — this
//! plan's task 11 measured persist wall-clock only, never peak memory,
//! and set no `lru` anywhere in this module. Plan 9b's peak-memory
//! measurement owns that call.

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
    /// Member-tree lookups answered from the pack.
    pub member_tree_hits: AtomicU64,
    /// Member-tree lookups the pack could not answer.
    pub member_tree_misses: AtomicU64,
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
    /// Wall-clock milliseconds `cache::persist` spent, accumulated
    /// across every call in the session (plan 9a, task 11). Read with
    /// `std::time::Instant` at the persist orchestration layer only,
    /// never inside a salsa query: this is telemetry for the stats
    /// line, and never feeds analysis or the rendered diagnostics.
    pub persist_milliseconds: AtomicU64,
    /// Typed-signature lookups (plan 9a, task 8) that found a recorded
    /// entry under the queried key — presence alone, counted at
    /// `SnapshotCache`, never the query-layer validation outcome
    /// (counters are forbidden inside a salsa query).
    pub signatures_found: AtomicU64,
    /// Typed-signature lookups with no recorded entry under the queried
    /// key.
    pub signatures_absent: AtomicU64,
    /// Files whose typed half (plan 9a, task 9) was served from the
    /// cache: present, every class and function digest unchanged, every
    /// inferred edge's live return unchanged — no body walked, no
    /// inference ran.
    pub typed_served: AtomicU64,
    /// Files whose typed half was recomputed: absent, stale, or the
    /// untyped half itself discarded (which takes the typed half down
    /// with it). Counted at the same fork `typed_served` is, in
    /// `analysis::served_typed_diagnostics` — the orchestration layer,
    /// never inside a query.
    pub typed_recomputed: AtomicU64,
}

impl CacheStatistics {
    /// The one-line summary the environment variable asks for.
    ///
    /// The persist clause carries its accumulated duration (task 11)
    /// only when `persist_milliseconds` is positive: a session that
    /// never persisted (or ran before the instrument, for any stored
    /// baseline) prints exactly the clause it always did, and the
    /// figure never becomes a clause of its own (decision 13).
    pub fn render(&self) -> String {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        let persist_milliseconds = load(&self.persist_milliseconds);
        let persist_clause = if persist_milliseconds > 0 {
            format!(
                "persist {} written / {} skipped / {} failed, {}ms",
                load(&self.persist_written),
                load(&self.persist_skipped),
                load(&self.persist_failed),
                persist_milliseconds,
            )
        } else {
            format!(
                "persist {} written / {} skipped / {} failed",
                load(&self.persist_written),
                load(&self.persist_skipped),
                load(&self.persist_failed),
            )
        };
        format!(
            "cache: trees {} hit / {} miss; members {} hit / {} miss; verdicts {} served / {} discarded / {} absent; typed {} bodies, edges {} declared / {} inferred / {} provider, verdicts {} served / {} recomputed; {}",
            load(&self.tree_hits),
            load(&self.tree_misses),
            load(&self.member_tree_hits),
            load(&self.member_tree_misses),
            load(&self.verdicts_served),
            load(&self.verdicts_discarded),
            load(&self.verdicts_absent),
            load(&self.typed_bodies),
            load(&self.typed_declared_edges),
            load(&self.typed_inferred_edges),
            load(&self.typed_provider_edges),
            load(&self.typed_served),
            load(&self.typed_recomputed),
            persist_clause,
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
        statistics.member_tree_hits.fetch_add(2, Ordering::Relaxed);
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
        statistics.typed_served.fetch_add(8, Ordering::Relaxed);
        statistics.typed_recomputed.fetch_add(9, Ordering::Relaxed);
        statistics.persist_failed.fetch_add(1, Ordering::Relaxed);
        assert_eq!(
            statistics.render(),
            "cache: trees 3 hit / 0 miss; members 2 hit / 0 miss; verdicts 2 served / 0 discarded / 0 absent; typed 4 bodies, edges 5 declared / 6 inferred / 7 provider, verdicts 8 served / 9 recomputed; persist 0 written / 0 skipped / 1 failed",
        );
    }

    #[test]
    fn the_persist_clause_carries_its_duration_when_positive() {
        let statistics = CacheStatistics::default();
        statistics.persist_written.fetch_add(2, Ordering::Relaxed);
        statistics.persist_skipped.fetch_add(1, Ordering::Relaxed);
        statistics
            .persist_milliseconds
            .fetch_add(37, Ordering::Relaxed);
        assert!(
            statistics
                .render()
                .contains("persist 2 written / 1 skipped / 0 failed, 37ms"),
            "the duration is folded into the existing persist clause, not a \
             separate one: {}",
            statistics.render(),
        );
    }

    #[test]
    fn the_persist_clause_omits_the_duration_when_zero() {
        let statistics = CacheStatistics::default();
        statistics.persist_written.fetch_add(2, Ordering::Relaxed);
        assert!(
            statistics
                .render()
                .contains("persist 2 written / 0 skipped / 0 failed"),
        );
        assert!(
            !statistics.render().contains("ms"),
            "no timing was ever recorded, so no duration figure appears: {}",
            statistics.render(),
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
