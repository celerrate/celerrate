//! Revalidation: a persisted verdict may speak for a file again only
//! when the file's bytes hash to its key (checked by the map lookup)
//! and every recorded resolution answer still holds against the
//! current database. The re-resolution goes through the same memoized
//! per-name lookups the checks use, so a pass that validates many
//! entries pays each name once.
//!
//! **Layered validation.** The untyped half validates
//! first, exactly as it always has: any record whose answer moved
//! discards the WHOLE entry (`VerdictLookup::Discarded`), typed half
//! included — an untyped miss has nothing left to layer over. Only once
//! every untyped record survives does the typed half get its own,
//! independent verdict (`TypedOutcome`): present, every consulted
//! class's and function's digest unchanged, and every recorded inferred
//! edge's callee still answering the same return through the LIVE
//! `inferred_function_return`/`inferred_method_return` queries — which
//! themselves serve warm through the per-signature cache when
//! their own entries validate, so a served typed half costs
//! microseconds, not a body walk. A partial hit (untyped served, typed
//! recomputed) is a first-class outcome, not a fallback: the two halves
//! are independent artifact classes that happen to share one entry.

use std::collections::HashMap;

use celerrate_db::SourceFile;
use celerrate_semantics::{
    ClassQuery, SymbolSources, UseTables, answer_of, item_tree, resolve_name,
};
use celerrate_types::{
    FunctionQuery, MethodQuery, StoredSignatureKey, class_surface_digest,
    function_signature_digest, inferred_function_return, inferred_method_return,
};

use crate::analysis::AnalysisInputs;

use super::stored::StoredVerdict;

/// The typed half's own layered outcome, computed only
/// once the untyped half already validated. `Served` when
/// `StoredVerdict.typed` is present and every one of its records
/// re-checks clean against the live project; `Recompute` otherwise — no
/// persisted typed portion at all (the lever was off, or the file was
/// ineligible, at persist time), a consulted class's or function's
/// surface moved, or a recorded inferred edge's callee now answers a
/// different return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedOutcome {
    Served,
    Recompute,
}

/// What the diagnostics pack answers for one file. The three cases are
/// distinct because the statistics distinguish them: a `Discarded` is
/// revalidation doing its job, an `Absent` is an ordinary cold miss. A
/// `Hit` carries the typed half's own, independently computed
/// [`TypedOutcome`] alongside the verdict: the untyped half validating
/// says nothing about the typed half's own records, which is exactly
/// the partial-hit outcome this module's doc describes.
pub enum VerdictLookup<'a> {
    /// The untyped half is present and every record revalidated: it may
    /// speak. The typed half's own outcome is layered on top,
    /// independently.
    Hit {
        verdict: &'a StoredVerdict,
        typed: TypedOutcome,
    },
    /// Present, but a recorded answer no longer holds: recompute both
    /// halves.
    Discarded,
    /// No entry under this content hash: recompute both halves.
    Absent,
}

/// Looks the file's verdict up and revalidates it, layer by layer.
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
    let typed = validate_typed(inputs, stored);
    VerdictLookup::Hit {
        verdict: stored,
        typed,
    }
}

/// The typed half's own validation: absent when nothing was persisted
/// for it, otherwise every class digest, function digest, and
/// inferred edge re-checked against the live project. Every read here
/// is a salsa-tracked query call, exactly like
/// `celerrate_types::inference::validated_stored_return`'s own
/// recursive revalidation this reuses for the inferred-edge check, a
/// live inferred return that itself serves warm through the fourth
/// pack's per-signature cache costs microseconds, not a body walk.
fn validate_typed(inputs: &AnalysisInputs, stored: &StoredVerdict) -> TypedOutcome {
    let Some(typed) = &stored.typed else {
        return TypedOutcome::Recompute;
    };
    let database = &inputs.database;
    for class in &typed.classes {
        let current = class_surface_digest(
            database,
            inputs.files,
            inputs.stubs,
            inputs.configuration,
            ClassQuery::new(database, class.key.clone()),
        );
        if current != class.digest {
            return TypedOutcome::Recompute;
        }
    }
    for function in &typed.functions {
        let current = function_signature_digest(
            database,
            inputs.files,
            inputs.stubs,
            inputs.configuration,
            FunctionQuery::new(database, function.key.clone()),
        );
        if current != function.digest {
            return TypedOutcome::Recompute;
        }
    }
    for edge in &typed.inferred {
        let live = match &edge.callee {
            StoredSignatureKey::Function { key } => inferred_function_return(
                database,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
                FunctionQuery::new(database, key.clone()),
            ),
            StoredSignatureKey::Method {
                class_key,
                member_key,
            } => inferred_method_return(
                database,
                inputs.files,
                inputs.stubs,
                inputs.configuration,
                MethodQuery::new(database, class_key.clone(), member_key.clone()),
            ),
        };
        if Some(live) != edge.return_type.to_type_id(database) {
            return TypedOutcome::Recompute;
        }
    }
    TypedOutcome::Served
}
