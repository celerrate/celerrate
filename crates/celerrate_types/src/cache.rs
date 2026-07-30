//! The typed-artifact-cache extension point: the
//! dependency-inverted trait this layer consults for a persisted
//! inferred signature, the handle the composition root registers as a
//! salsa singleton input, and nothing about disks. The persistent cache
//! is a CLI concern (`celerrate_cli::cache::snapshot::SnapshotCache`);
//! this layer only asks "is a signature already known for this key?" —
//! `crate::inference::validated_stored_return` is what turns a `Some`
//! answer into a served [`crate::representation::TypeId`], re-checking
//! every fact it depends on through live salsa reads before trusting it.
//!
//! Mirrors [`celerrate_semantics::cache::ArtifactCache`]'s shape
//! exactly (trait, `Clone` handle, `#[salsa::input(singleton)]`), one
//! layer up: that cache answers "what is this file's lowered shape?";
//! this one answers "what did this signature infer to?".

use std::fmt;
use std::sync::Arc;

use crate::stored::{StoredInferredSignature, StoredSignatureKey};

/// What a registered typed-artifact cache can answer: the persisted
/// inferred signature recorded under one [`StoredSignatureKey`], or
/// `None` when nothing was recorded for it. A `Some` answer is not
/// trusted as-is — `crate::inference::validated_stored_return` is what
/// revalidates it against live salsa facts before ever serving its
/// `return_type` — so an implementation may answer anything under this
/// trait alone; the exactness contract lives at the validation site, not
/// here.
pub trait TypedArtifactCache: Send + Sync {
    /// The cached inferred signature recorded under `key`, if any.
    fn inferred_signature(&self, key: &StoredSignatureKey) -> Option<StoredInferredSignature>;
}

/// The registered cache, as the cloneable handle a salsa input field
/// requires.
#[derive(Clone)]
pub struct TypedCacheHandle(pub Arc<dyn TypedArtifactCache>);

impl fmt::Debug for TypedCacheHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("TypedCacheHandle").finish()
    }
}

/// The singleton input the composition root registers once, before any
/// query runs, and never mutates: reading it therefore never
/// invalidates anything. Databases that register nothing (every test
/// database that does not opt in) take the compute path through
/// `try_get`'s `None`.
#[salsa::input(singleton)]
pub struct TypedCacheInput {
    #[returns(ref)]
    pub cache: TypedCacheHandle,
}
