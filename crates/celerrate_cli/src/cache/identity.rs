//! The binary identity the pack header carries: the blake3 hash of the
//! running executable's own bytes, computed once per process. Two
//! different binaries never accept each other's packs, mechanically —
//! no human-remembered version bump involved (audit finding I1: keying
//! on `CARGO_PKG_VERSION` alone let every development rebuild within
//! one version serve the previous build's stale packs). When the
//! executable cannot be found or read, the identity falls back to the
//! crate version: the pre-hash behavior, never a failure.

use std::sync::OnceLock;

/// The identity of `bytes` as a pack header carries it: blake3, hex.
fn identity_of(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// This process's binary identity, computed once and cached.
pub fn binary_identity() -> &'static str {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|path| std::fs::read(path).ok())
            .map(|bytes| identity_of(&bytes))
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::{binary_identity, identity_of};

    #[test]
    fn the_identity_is_the_blake3_hex_of_the_bytes() {
        assert_eq!(
            identity_of(b"payload"),
            blake3::hash(b"payload").to_hex().to_string(),
        );
        assert_eq!(identity_of(b"payload").len(), 64);
    }

    /// The fallback branch (`current_exe` failing) cannot be driven from
    /// a test: the test binary exists and is readable by construction.
    /// What can be pinned is that the fallback did NOT fire here — the
    /// identity is a 64-character hash, not a version string — and that
    /// repeated calls answer the same interned value.
    #[test]
    fn the_binary_identity_is_stable_and_hash_shaped() {
        let first = binary_identity();
        assert_eq!(first, binary_identity());
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
    }
}
