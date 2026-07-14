//! The pack file: `magic ++ blake3 checksum of the payload ++ payload`,
//! the payload being one postcard-encoded `Pack`. Every rejection —
//! short file, wrong magic, checksum mismatch, undecodable payload,
//! header mismatch — answers `None`, and the caller regenerates:
//! corruption is detected, never fatal, never visible.

use std::path::Path;

use celerrate_project::PhpVersionRange;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The first eight bytes of every pack file.
pub const CACHE_MAGIC: [u8; 8] = *b"CELCACHE";

/// Bumped on a deliberate break of the stored shapes. The header also
/// carries the binary's own content hash, so any rebuild already
/// discards packs on its own; this constant is no longer what protects
/// development builds (the self-hash carries that), it is the named,
/// reviewable record of deliberate format breaks.
///
/// 2: `StoredItemTree` gained `defines`, carrying `define()`-detected
/// constant names into the item-tree pack (see
/// `celerrate_semantics::items`'s module doc). A pack written under
/// schema 1 has no such field and must be discarded wholesale, exactly
/// like any other header mismatch: there is no migration, only a cold
/// rebuild.
pub const CACHE_SCHEMA_VERSION: u32 = 2;

/// What must match for a pack to be readable at all: the schema, the
/// binary, the stub content, and the PHP version range. Any mismatch
/// discards the whole pack, so entry keys only need to encode what
/// varies within one configuration — file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct PackHeader {
    pub schema: u32,
    pub binary: String,
    /// The blake3 hash of the embedded stub blob: pins the stub
    /// *content*, not just its format — a new snapshot changes
    /// availability answers.
    pub stub_blob: [u8; 32],
    pub php_minimum: (u8, u8),
    pub php_maximum: (u8, u8),
}

impl PackHeader {
    /// The header of this binary analyzing under `range`.
    pub fn current(range: PhpVersionRange) -> Self {
        Self {
            schema: CACHE_SCHEMA_VERSION,
            binary: super::identity::binary_identity().to_owned(),
            stub_blob: *blake3::hash(celerrate_stubs::EMBEDDED_STUB_BLOB).as_bytes(),
            php_minimum: (range.minimum.major, range.minimum.minor),
            php_maximum: (range.maximum.major, range.maximum.minor),
        }
    }
}

/// One pack: its header and its entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Pack<Entries> {
    pub header: PackHeader,
    pub entries: Entries,
}

/// Encodes a pack into its on-disk bytes. `None` only if postcard
/// cannot serialize the value, which no stored shape can trigger; the
/// caller skips the write rather than failing the run.
pub fn encode<Entries: Serialize>(pack: &Pack<Entries>) -> Option<Vec<u8>> {
    let payload = postcard::to_stdvec(pack).ok()?;
    let mut bytes = Vec::with_capacity(CACHE_MAGIC.len() + 32 + payload.len());
    bytes.extend_from_slice(&CACHE_MAGIC);
    bytes.extend_from_slice(blake3::hash(&payload).as_bytes());
    bytes.extend_from_slice(&payload);
    Some(bytes)
}

/// Decodes and validates a pack, or answers `None` for anything less
/// than a whole, current, matching file.
pub fn decode<Entries: DeserializeOwned>(
    bytes: &[u8],
    expected: &PackHeader,
) -> Option<Pack<Entries>> {
    let magic = bytes.get(..CACHE_MAGIC.len())?;
    if magic != CACHE_MAGIC {
        return None;
    }
    let checksum = bytes.get(CACHE_MAGIC.len()..CACHE_MAGIC.len() + 32)?;
    let payload = bytes.get(CACHE_MAGIC.len() + 32..)?;
    if blake3::hash(payload).as_bytes() != checksum {
        return None;
    }
    let pack: Pack<Entries> = postcard::from_bytes(payload).ok()?;
    (pack.header == *expected).then_some(pack)
}

/// The prefix `write_atomically`'s temporary files carry. Named explicitly,
/// rather than left to `tempfile`'s crate-default, because two other places
/// match it by literal: `sweep_crash_debris` (crash debris left behind in
/// `.celerrate/cache/`) and `prepare_directory`'s sweep of `.celerrate/`
/// itself (the `.gitignore`'s temporary lands there, one level up).
pub(crate) const TEMPORARY_FILE_PREFIX: &str = ".tmp";

/// Writes bytes to `path` through a temporary file in the same
/// directory plus a rename, so a reader never sees a torn file and a
/// concurrent writer's last rename wins whole.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the pack path has no parent directory"))?;
    let mut file = tempfile::Builder::new()
        .prefix(TEMPORARY_FILE_PREFIX)
        .tempfile_in(directory)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_project::{PhpVersion, PhpVersionRange};

    use super::{CACHE_MAGIC, Pack, PackHeader, decode, encode, write_atomically};

    fn header() -> PackHeader {
        PackHeader::current(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
    }

    fn sample() -> Pack<Vec<(u32, String)>> {
        Pack {
            header: header(),
            entries: vec![(1, "one".to_owned()), (2, "two".to_owned())],
        }
    }

    #[test]
    fn a_pack_round_trips() {
        let bytes = encode(&sample()).unwrap();
        assert_eq!(&bytes[..8], &CACHE_MAGIC);
        let decoded: Pack<Vec<(u32, String)>> = decode(&bytes, &header()).unwrap();
        assert_eq!(decoded, sample());
    }

    #[test]
    fn every_corruption_mode_answers_none() {
        let bytes = encode(&sample()).unwrap();

        // Truncated: shorter than the magic, shorter than the
        // checksum, and mid-payload.
        for length in [0, 4, 20, bytes.len() - 3] {
            let truncated = &bytes[..length];
            assert!(
                decode::<Vec<(u32, String)>>(truncated, &header()).is_none(),
                "a pack truncated to {length} bytes must be rejected",
            );
        }

        // Wrong magic.
        let mut wrong_magic = bytes.clone();
        wrong_magic[0] = b'X';
        assert!(decode::<Vec<(u32, String)>>(&wrong_magic, &header()).is_none());

        // A flipped payload byte fails the checksum.
        let mut flipped = bytes.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 0xFF;
        assert!(decode::<Vec<(u32, String)>>(&flipped, &header()).is_none());

        // A flipped checksum byte fails the checksum.
        let mut bad_checksum = bytes.clone();
        bad_checksum[10] ^= 0xFF;
        assert!(decode::<Vec<(u32, String)>>(&bad_checksum, &header()).is_none());

        // Garbage of plausible length.
        let garbage = vec![0xAAu8; bytes.len()];
        assert!(decode::<Vec<(u32, String)>>(&garbage, &header()).is_none());
    }

    #[test]
    fn a_header_mismatch_discards_the_whole_pack() {
        let bytes = encode(&sample()).unwrap();
        let other_range = PackHeader::current(PhpVersionRange::new(
            PhpVersion::new(8, 2),
            PhpVersion::new(8, 5),
        ));
        assert!(decode::<Vec<(u32, String)>>(&bytes, &other_range).is_none());

        let mut other_schema = header();
        other_schema.schema += 1;
        assert!(decode::<Vec<(u32, String)>>(&bytes, &other_schema).is_none());

        let mut other_binary = header();
        other_binary.binary = "0.0.0-other".to_owned();
        assert!(decode::<Vec<(u32, String)>>(&bytes, &other_binary).is_none());

        let mut other_stub = header();
        other_stub.stub_blob[0] ^= 0xFF;
        assert!(
            decode::<Vec<(u32, String)>>(&bytes, &other_stub).is_none(),
            "the stub-blob field is load-bearing: a new snapshot changes availability answers",
        );
    }

    #[test]
    fn the_atomic_write_replaces_the_file_whole() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pack.bin");
        write_atomically(&path, b"first").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        write_atomically(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
    }

    /// Audit finding I1: keying the header on `CARGO_PKG_VERSION` alone
    /// let development rebuilds within one version accept each other's
    /// packs — stale messages served byte-for-byte, newly added rules
    /// silently missing. The header now carries the executable's own
    /// content hash: two different binaries never speak.
    #[test]
    fn the_header_carries_the_binary_self_hash() {
        assert_eq!(header().binary, super::super::identity::binary_identity());
    }

    /// The atomicity clause is about concurrency (audit finding I5):
    /// "a reader never sees a torn file and a concurrent writer's last
    /// rename wins whole". One writer alternates two payloads while
    /// this thread reads; every observed read must be byte-for-byte one
    /// of the two payloads and must decode whole.
    #[test]
    fn a_reader_racing_a_writer_never_sees_a_torn_pack() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pack.bin");
        let first = encode(&sample()).unwrap();
        let second = encode(&Pack {
            header: header(),
            entries: vec![(9, "nine".to_owned())],
        })
        .unwrap();
        write_atomically(&path, &first).unwrap();

        let writer_path = path.clone();
        let writer_first = first.clone();
        let writer_second = second.clone();
        let writer = std::thread::spawn(move || {
            for round in 0..200 {
                let bytes = if round % 2 == 0 {
                    &writer_second
                } else {
                    &writer_first
                };
                // A transiently failed write (e.g. Windows' delete-pending
                // window on the just-replaced file) is not what this test
                // pins; the assertions below on every observed read carry
                // the property.
                let _ = write_atomically(&writer_path, bytes);
            }
        });

        for _ in 0..200 {
            // An errored read observed nothing: on Windows this can happen
            // transiently while a concurrent rename is in flight, and it is
            // not a torn read.
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            assert!(
                bytes == first || bytes == second,
                "a read observed bytes that are neither payload: torn",
            );
            let decoded: Option<Pack<Vec<(u32, String)>>> = decode(&bytes, &header());
            assert!(decoded.is_some(), "every observed read decodes whole");
        }
        writer.join().unwrap();
    }
}
