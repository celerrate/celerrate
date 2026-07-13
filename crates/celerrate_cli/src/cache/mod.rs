//! The persistent artifact cache: a content-addressed derived-artifact
//! cache above salsa, persisted to `.celerrate/cache/` and used to
//! re-seed a fresh database at startup. Nothing here is ever fatal:
//! every failure mode of a cache file answers by recomputation.

pub mod pack;
pub mod snapshot;
pub mod stored;
