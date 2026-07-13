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

/// The stored verdict for `file`, if one exists under its content
/// address and every record revalidates. `None` means recompute.
pub fn validated_verdict(inputs: &AnalysisInputs, file: SourceFile) -> Option<&StoredVerdict> {
    let database = &inputs.database;
    let stored = inputs
        .cache
        .verdicts
        .get(&celerrate_db::content_hash(database, file))?;
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
            return None;
        }
    }
    Some(stored)
}
