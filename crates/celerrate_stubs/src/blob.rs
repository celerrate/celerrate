//! The hand-written stub blob format: versioned, checksummed,
//! sectioned, byte-deterministic. The reader is tolerant end to end:
//! corruption yields a [`StubBlobError`], never a panic.

use core::fmt;

use celerrate_project::PhpVersion;

use crate::index::StubIndex;
use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};

pub const BLOB_MAGIC: [u8; 8] = *b"CELSTUBS";

/// Bumped only on incompatible layout changes. Additive evolution goes
/// through new sections, which old readers skip.
pub const BLOB_FORMAT_VERSION: u32 = 1;

/// The top-level symbol table: the one live section.
pub const SECTION_SYMBOL_TABLE: u32 = 1;

/// Reserved: per-version signature deltas (sub-project 3).
pub const SECTION_SIGNATURES: u32 = 2;

/// Reserved: the overlay merge point (Celerrate refinements, plugin
/// stubs).
pub const SECTION_OVERLAYS: u32 = 3;

/// Why a blob failed to decode. Every variant is a clean rejection:
/// the composition root falls back to an empty index and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StubBlobError {
    TooShort,
    BadMagic,
    UnsupportedFormatVersion(u32),
    ChecksumMismatch,
    MissingSymbolTable,
    MalformedSection,
}

impl fmt::Display for StubBlobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(formatter, "stub blob is truncated"),
            Self::BadMagic => write!(formatter, "not a Celerrate stub blob"),
            Self::UnsupportedFormatVersion(version) => {
                write!(formatter, "unsupported stub blob format version {version}")
            }
            Self::ChecksumMismatch => write!(formatter, "stub blob checksum mismatch"),
            Self::MissingSymbolTable => {
                write!(formatter, "stub blob carries no symbol table section")
            }
            Self::MalformedSection => write!(formatter, "malformed stub blob section"),
        }
    }
}

impl std::error::Error for StubBlobError {}

/// Encodes the index. Deterministic: the same index always produces
/// the same bytes (the index is already sorted and merged).
pub fn encode(index: &StubIndex) -> Vec<u8> {
    let symbol_table = encode_symbol_table(index);
    let table_entries = 1u32;
    let payload_offset = 24u64 + u64::from(table_entries) * 20;
    let mut blob = Vec::with_capacity(symbol_table.len() + 64);
    blob.extend_from_slice(&BLOB_MAGIC);
    blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
    blob.extend_from_slice(&[0; 8]); // checksum, patched below
    blob.extend_from_slice(&table_entries.to_le_bytes());
    blob.extend_from_slice(&SECTION_SYMBOL_TABLE.to_le_bytes());
    blob.extend_from_slice(&payload_offset.to_le_bytes());
    blob.extend_from_slice(&(symbol_table.len() as u64).to_le_bytes());
    blob.extend_from_slice(&symbol_table);
    let checksum = fnv1a64(blob.get(20..).unwrap_or_default());
    if let Some(slot) = blob.get_mut(12..20) {
        slot.copy_from_slice(&checksum.to_le_bytes());
    }
    blob
}

fn encode_symbol_table(index: &StubIndex) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(index.len() as u32).to_le_bytes());
    for symbol in index.symbols() {
        bytes.push(symbol.kind.as_u8());
        let availability = symbol.availability;
        let since = availability
            .deprecated
            .and_then(|deprecation| deprecation.since);
        let mut flags = 0u8;
        if availability.introduced.is_some() {
            flags |= 1;
        }
        if availability.removed.is_some() {
            flags |= 1 << 1;
        }
        if availability.deprecated.is_some() {
            flags |= 1 << 2;
        }
        if since.is_some() {
            flags |= 1 << 3;
        }
        bytes.push(flags);
        for version in [availability.introduced, availability.removed, since]
            .into_iter()
            .flatten()
        {
            bytes.extend_from_slice(&[version.major, version.minor]);
        }
        bytes.extend_from_slice(&(symbol.name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(symbol.name.as_bytes());
    }
    bytes
}

/// Decodes a blob. Tolerant: every malformation is an error value.
pub fn decode(blob: &[u8]) -> Result<StubIndex, StubBlobError> {
    let mut header = Reader::new(blob);
    let magic = header.take(8).ok_or(StubBlobError::TooShort)?;
    if magic != BLOB_MAGIC {
        return Err(StubBlobError::BadMagic);
    }
    let format_version = header.u32().ok_or(StubBlobError::TooShort)?;
    if format_version != BLOB_FORMAT_VERSION {
        return Err(StubBlobError::UnsupportedFormatVersion(format_version));
    }
    let checksum = header.u64().ok_or(StubBlobError::TooShort)?;
    let checksummed = blob.get(20..).ok_or(StubBlobError::TooShort)?;
    if fnv1a64(checksummed) != checksum {
        return Err(StubBlobError::ChecksumMismatch);
    }
    let section_count = header.u32().ok_or(StubBlobError::TooShort)?;
    let mut symbol_table: Option<&[u8]> = None;
    for _ in 0..section_count {
        let identifier = header.u32().ok_or(StubBlobError::TooShort)?;
        let offset = header.u64().ok_or(StubBlobError::TooShort)?;
        let length = header.u64().ok_or(StubBlobError::TooShort)?;
        let end = offset
            .checked_add(length)
            .ok_or(StubBlobError::MalformedSection)?;
        let start = usize::try_from(offset).map_err(|_| StubBlobError::MalformedSection)?;
        let end = usize::try_from(end).map_err(|_| StubBlobError::MalformedSection)?;
        let section = blob
            .get(start..end)
            .ok_or(StubBlobError::MalformedSection)?;
        if identifier == SECTION_SYMBOL_TABLE {
            symbol_table = Some(section);
        }
        // Unknown identifiers are skipped: newer blobs that only add
        // sections stay readable without a format version bump.
    }
    decode_symbol_table(symbol_table.ok_or(StubBlobError::MissingSymbolTable)?)
}

fn decode_symbol_table(bytes: &[u8]) -> Result<StubIndex, StubBlobError> {
    let mut reader = Reader::new(bytes);
    let count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut symbols = Vec::new();
    for _ in 0..count {
        let kind = reader
            .u8()
            .and_then(StubSymbolKind::from_u8)
            .ok_or(StubBlobError::MalformedSection)?;
        let flags = reader.u8().ok_or(StubBlobError::MalformedSection)?;
        let introduced = if flags & 1 != 0 {
            Some(reader.version().ok_or(StubBlobError::MalformedSection)?)
        } else {
            None
        };
        let removed = if flags & (1 << 1) != 0 {
            Some(reader.version().ok_or(StubBlobError::MalformedSection)?)
        } else {
            None
        };
        let since = if flags & (1 << 3) != 0 {
            Some(reader.version().ok_or(StubBlobError::MalformedSection)?)
        } else {
            None
        };
        let deprecated = (flags & (1 << 2) != 0).then_some(StubDeprecation { since });
        let name_length = reader.u32().ok_or(StubBlobError::MalformedSection)?;
        let name_length =
            usize::try_from(name_length).map_err(|_| StubBlobError::MalformedSection)?;
        let name_bytes = reader
            .take(name_length)
            .ok_or(StubBlobError::MalformedSection)?;
        let name = core::str::from_utf8(name_bytes)
            .map_err(|_| StubBlobError::MalformedSection)?
            .to_owned();
        symbols.push(StubSymbol {
            name,
            kind,
            availability: StubAvailability {
                introduced,
                removed,
                deprecated,
            },
        });
    }
    Ok(StubIndex::from_symbols(symbols))
}

/// FNV-1a, 64-bit: six lines beat a checksum dependency.
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A cursor over borrowed bytes; every read is checked, no indexing.
struct Reader<'blob> {
    bytes: &'blob [u8],
}

impl<'blob> Reader<'blob> {
    fn new(bytes: &'blob [u8]) -> Self {
        Self { bytes }
    }

    fn take(&mut self, count: usize) -> Option<&'blob [u8]> {
        let (head, tail) = self.bytes.split_at_checked(count)?;
        self.bytes = tail;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn version(&mut self) -> Option<PhpVersion> {
        let bytes = self.take(2)?;
        Some(PhpVersion::new(*bytes.first()?, *bytes.get(1)?))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_project::PhpVersion;

    use super::{
        BLOB_FORMAT_VERSION, BLOB_MAGIC, SECTION_SYMBOL_TABLE, StubBlobError, decode, encode,
        fnv1a64,
    };
    use crate::index::StubIndex;
    use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};

    fn sample_index() -> StubIndex {
        StubIndex::from_symbols(vec![
            StubSymbol {
                name: "Random\\Randomizer".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability {
                    introduced: Some(PhpVersion::new(8, 2)),
                    removed: None,
                    deprecated: None,
                },
            },
            StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "utf8_encode".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability {
                    introduced: None,
                    removed: Some(PhpVersion::new(8, 4)),
                    deprecated: Some(StubDeprecation {
                        since: Some(PhpVersion::new(8, 2)),
                    }),
                },
            },
            StubSymbol {
                name: "E_ALL".to_owned(),
                kind: StubSymbolKind::Constant,
                availability: StubAvailability {
                    introduced: None,
                    removed: None,
                    deprecated: Some(StubDeprecation { since: None }),
                },
            },
        ])
    }

    #[test]
    fn an_index_round_trips_through_the_blob() {
        let index = sample_index();
        assert_eq!(decode(&encode(&index)), Ok(index));
    }

    #[test]
    fn the_empty_index_round_trips() {
        let index = StubIndex::default();
        assert_eq!(decode(&encode(&index)), Ok(index));
    }

    #[test]
    fn encoding_is_deterministic() {
        assert_eq!(encode(&sample_index()), encode(&sample_index()));
    }

    #[test]
    fn the_blob_starts_with_magic_and_format_version() {
        let blob = encode(&StubIndex::default());
        assert_eq!(blob[0..8], BLOB_MAGIC);
        assert_eq!(blob[8..12], BLOB_FORMAT_VERSION.to_le_bytes());
    }

    #[test]
    fn an_empty_input_is_too_short() {
        assert_eq!(decode(&[]), Err(StubBlobError::TooShort));
    }

    #[test]
    fn a_foreign_blob_is_rejected_by_magic() {
        let mut blob = encode(&sample_index());
        blob[0] = b'X';
        assert_eq!(decode(&blob), Err(StubBlobError::BadMagic));
    }

    #[test]
    fn an_unknown_format_version_is_rejected_before_anything_else_is_read() {
        let mut blob = encode(&sample_index());
        blob[8..12].copy_from_slice(&999u32.to_le_bytes());
        assert_eq!(
            decode(&blob),
            Err(StubBlobError::UnsupportedFormatVersion(999)),
        );
    }

    #[test]
    fn a_flipped_payload_byte_fails_the_checksum() {
        let mut blob = encode(&sample_index());
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        assert_eq!(decode(&blob), Err(StubBlobError::ChecksumMismatch));
    }

    #[test]
    fn a_truncated_blob_never_panics() {
        let blob = encode(&sample_index());
        for length in 0..blob.len() {
            // Every prefix decodes to a clean error, never a panic.
            assert!(decode(&blob[..length]).is_err(), "prefix length {length}");
        }
    }

    #[test]
    fn unknown_sections_are_skipped_for_forward_compatibility() {
        // Hand-build a version-1 blob whose table carries an unknown
        // section before the symbol table.
        let symbol_table = {
            let encoded = encode(&sample_index());
            // The symbol table of a freshly encoded blob starts right
            // after the header (24) plus one 20-byte table entry.
            encoded[44..].to_vec()
        };
        let unknown_payload = b"future data";
        let table_entries = 2u32;
        let unknown_offset = 24u64 + u64::from(table_entries) * 20;
        let symbol_offset = unknown_offset + unknown_payload.len() as u64;
        let mut blob = Vec::new();
        blob.extend_from_slice(&BLOB_MAGIC);
        blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
        blob.extend_from_slice(&[0; 8]);
        blob.extend_from_slice(&table_entries.to_le_bytes());
        blob.extend_from_slice(&777u32.to_le_bytes());
        blob.extend_from_slice(&unknown_offset.to_le_bytes());
        blob.extend_from_slice(&(unknown_payload.len() as u64).to_le_bytes());
        blob.extend_from_slice(&SECTION_SYMBOL_TABLE.to_le_bytes());
        blob.extend_from_slice(&symbol_offset.to_le_bytes());
        blob.extend_from_slice(&(symbol_table.len() as u64).to_le_bytes());
        blob.extend_from_slice(unknown_payload);
        blob.extend_from_slice(&symbol_table);
        let checksum = fnv1a64(&blob[20..]);
        blob[12..20].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(decode(&blob), Ok(sample_index()));
    }

    #[test]
    fn a_blob_without_a_symbol_table_reports_it() {
        let mut blob = Vec::new();
        blob.extend_from_slice(&BLOB_MAGIC);
        blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
        blob.extend_from_slice(&[0; 8]);
        blob.extend_from_slice(&0u32.to_le_bytes());
        let checksum = fnv1a64(&blob[20..]);
        blob[12..20].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(decode(&blob), Err(StubBlobError::MissingSymbolTable));
    }

    #[test]
    fn errors_render_for_humans() {
        assert_eq!(
            StubBlobError::UnsupportedFormatVersion(7).to_string(),
            "unsupported stub blob format version 7",
        );
        assert!(!StubBlobError::ChecksumMismatch.to_string().is_empty());
    }
}
