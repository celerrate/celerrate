//! Revalidation: a persisted verdict may speak for a file again only
//! when the file's bytes hash to its key (checked by the map lookup)
//! and every recorded resolution answer still holds against the
//! current database. The re-resolution goes through the same memoized
//! per-name lookups the checks use, so a pass that validates many
//! entries pays each name once.

use std::collections::HashMap;

use celerrate_db::SourceFile;
use celerrate_semantics::{SymbolSources, UseTables, answer_of, item_tree, resolve_name};

use crate::analysis::AnalysisInputs;

use super::stored::StoredVerdict;

/// What the diagnostics pack answers for one file. The three cases are
/// distinct because the statistics distinguish them: a `Discarded` is
/// revalidation doing its job, an `Absent` is an ordinary cold miss.
pub enum VerdictLookup<'a> {
    /// Present and every record revalidated: the verdict may speak.
    Hit(&'a StoredVerdict),
    /// Present, but a recorded answer no longer holds: recompute.
    Discarded,
    /// No entry under this content hash: recompute.
    Absent,
}

/// Looks the file's verdict up and revalidates it.
pub fn lookup_verdict(inputs: &AnalysisInputs, file: SourceFile) -> VerdictLookup<'_> {
    let database = &inputs.database;
    let Some(stored) = inputs
        .cache
        .verdicts
        .get(&celerrate_db::content_hash(database, file))
    else {
        return VerdictLookup::Absent;
    };
    let sources = SymbolSources {
        files: inputs.files,
        stubs: inputs.stubs,
        configuration: inputs.configuration,
    };
    let tree = item_tree(database, file);
    let mut tables_by_namespace: HashMap<&str, UseTables> = HashMap::new();
    for record in &stored.records {
        let tables = tables_by_namespace
            .entry(record.namespace.as_str())
            .or_insert_with(|| UseTables::for_namespace(tree, &record.namespace));
        let answer = answer_of(resolve_name(
            database,
            sources,
            &record.namespace,
            tables,
            &record.written,
            record.space(),
        ));
        if !record.matches(answer) {
            return VerdictLookup::Discarded;
        }
    }
    VerdictLookup::Hit(stored)
}

/// The stored verdict if it may speak; `None` means recompute. This is
/// the persist path's mirror of the pass's decision, deliberately
/// without statistics attached: only the pass itself counts, or
/// `persist`'s re-lookup would double-count every file.
pub fn validated_verdict(inputs: &AnalysisInputs, file: SourceFile) -> Option<&StoredVerdict> {
    match lookup_verdict(inputs, file) {
        VerdictLookup::Hit(stored) => Some(stored),
        VerdictLookup::Discarded | VerdictLookup::Absent => None,
    }
}
