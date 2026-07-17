//! The hand-written stub blob format: versioned, checksummed,
//! sectioned, byte-deterministic. The reader is tolerant end to end:
//! corruption yields a [`StubBlobError`], never a panic.

use core::fmt;

use celerrate_project::PhpVersion;

use crate::index::StubIndex;
use crate::signature::{
    StubClassSurface, StubMember, StubMemberKind, StubParameter, StubSignature, StubVisibility,
    VersionedTypeText,
};
use crate::symbol::{StubAvailability, StubDeprecation, StubSymbol, StubSymbolKind};

pub const BLOB_MAGIC: [u8; 8] = *b"CELSTUBS";

/// Version 2 marks the overlays section (`SECTION_OVERLAYS`) going
/// live: the schema bump the design's section 9 mandates whenever a
/// reserved section starts being written. Otherwise additive
/// evolution goes through new sections, which old readers skip
/// without a version bump.
pub const BLOB_FORMAT_VERSION: u32 = 2;

/// The top-level symbol table: the one live section.
pub const SECTION_SYMBOL_TABLE: u32 = 1;

/// Reserved: per-version signature deltas (sub-project 3).
pub const SECTION_SIGNATURES: u32 = 2;

/// The overlay merge point: live for the Celerrate refinements payload
/// (design section 7); still reserved for plugin stubs.
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
    let signatures = encode_signatures(index);
    let mut overlays = Vec::new();
    crate::refinements::encode_refinements(index.refinements(), &mut overlays);
    let table_entries = 3u32;
    let symbol_offset = 24u64 + u64::from(table_entries) * 20;
    let signature_offset = symbol_offset + symbol_table.len() as u64;
    let overlays_offset = signature_offset + signatures.len() as u64;
    let mut blob = Vec::with_capacity(symbol_table.len() + signatures.len() + overlays.len() + 64);
    blob.extend_from_slice(&BLOB_MAGIC);
    blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
    blob.extend_from_slice(&[0; 8]); // checksum, patched below
    blob.extend_from_slice(&table_entries.to_le_bytes());
    blob.extend_from_slice(&SECTION_SYMBOL_TABLE.to_le_bytes());
    blob.extend_from_slice(&symbol_offset.to_le_bytes());
    blob.extend_from_slice(&(symbol_table.len() as u64).to_le_bytes());
    blob.extend_from_slice(&SECTION_SIGNATURES.to_le_bytes());
    blob.extend_from_slice(&signature_offset.to_le_bytes());
    blob.extend_from_slice(&(signatures.len() as u64).to_le_bytes());
    blob.extend_from_slice(&SECTION_OVERLAYS.to_le_bytes());
    blob.extend_from_slice(&overlays_offset.to_le_bytes());
    blob.extend_from_slice(&(overlays.len() as u64).to_le_bytes());
    blob.extend_from_slice(&symbol_table);
    blob.extend_from_slice(&signatures);
    blob.extend_from_slice(&overlays);
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
        write_availability(&mut bytes, symbol.availability);
        write_string(&mut bytes, &symbol.name);
    }
    bytes
}

/// Writes the availability flag byte plus each present version: the
/// scheme shared by symbols, parameters, and members. `bit0`
/// introduced, `bit1` removed, `bit2` deprecated, `bit3` since.
fn write_availability(bytes: &mut Vec<u8>, availability: StubAvailability) {
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
}

fn write_string(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
}

fn write_versioned_text(bytes: &mut Vec<u8>, text: &VersionedTypeText) {
    match &text.default {
        Some(default) => {
            bytes.push(1);
            write_string(bytes, default);
        }
        None => bytes.push(0),
    }
    // `as u16`: overrides are keyed one per PHP minor version, and the
    // supported window (`SUPPORTED_VERSIONS`) spans five versions, so
    // this count cannot truncate in practice.
    bytes.extend_from_slice(&(text.overrides.len() as u16).to_le_bytes());
    for (version, override_text) in &text.overrides {
        bytes.extend_from_slice(&[version.major, version.minor]);
        write_string(bytes, override_text);
    }
}

fn write_signature(bytes: &mut Vec<u8>, signature: &StubSignature) {
    bytes.push(u8::from(signature.by_reference));
    write_versioned_text(bytes, &signature.return_type);
    bytes.extend_from_slice(&(signature.parameters.len() as u32).to_le_bytes());
    for parameter in &signature.parameters {
        let mut flags = 0u8;
        if parameter.optional {
            flags |= 1;
        }
        if parameter.by_reference {
            flags |= 1 << 1;
        }
        if parameter.variadic {
            flags |= 1 << 2;
        }
        bytes.push(flags);
        write_availability(bytes, parameter.availability);
        write_string(bytes, &parameter.name);
        write_versioned_text(bytes, &parameter.type_text);
    }
}

/// Encodes the signatures section: functions then classes, mirroring
/// the wire format documented on the module. Always produces bytes,
/// even for an empty index — the section is always written.
fn encode_signatures(index: &StubIndex) -> Vec<u8> {
    let mut bytes = Vec::new();
    // Bound once, outside both loops: a temporary
    // `&StubSignature::default()` at the call site would not live long
    // enough, and every method-less member shares the same empty
    // signature.
    let default_signature = StubSignature::default();
    bytes.extend_from_slice(&(index.functions().len() as u32).to_le_bytes());
    for (name, signature) in index.functions() {
        write_string(&mut bytes, name);
        write_signature(&mut bytes, signature);
    }
    bytes.extend_from_slice(&(index.classes().len() as u32).to_le_bytes());
    for (name, surface) in index.classes() {
        write_string(&mut bytes, name);
        bytes.extend_from_slice(&(surface.parents.len() as u32).to_le_bytes());
        for parent in &surface.parents {
            write_string(&mut bytes, parent);
        }
        bytes.extend_from_slice(&(surface.members.len() as u32).to_le_bytes());
        for member in &surface.members {
            bytes.push(member.kind.as_u8());
            let mut flags = 0u8;
            if member.is_static {
                flags |= 1;
            }
            flags |= member.visibility.as_u8() << 1;
            bytes.push(flags);
            write_availability(&mut bytes, member.availability);
            write_string(&mut bytes, &member.name);
            write_versioned_text(&mut bytes, &member.type_text);
            match &member.value_text {
                Some(value) => {
                    bytes.push(1);
                    write_string(&mut bytes, value);
                }
                None => bytes.push(0),
            }
            if member.kind == StubMemberKind::Method {
                let signature = member.signature.as_ref().unwrap_or(&default_signature);
                write_signature(&mut bytes, signature);
            }
        }
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
    let mut signatures: Option<&[u8]> = None;
    let mut overlays: Option<&[u8]> = None;
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
        } else if identifier == SECTION_SIGNATURES {
            signatures = Some(section);
        } else if identifier == SECTION_OVERLAYS {
            overlays = Some(section);
        }
        // Unknown identifiers are skipped: newer blobs that only add
        // sections stay readable without a format version bump.
    }
    let symbols = decode_symbol_table(symbol_table.ok_or(StubBlobError::MissingSymbolTable)?)?;
    let mut index = match signatures {
        Some(section) => {
            let (functions, classes) = decode_signatures(section)?;
            StubIndex::new(symbols, functions, classes)
        }
        // A blob without the signatures section (a pre-plan-3 blob, or
        // one that legitimately has nothing to say): empty payloads,
        // not an error — the section is optional on read.
        None => StubIndex::from_symbols(symbols),
    };
    if let Some(section) = overlays {
        index.set_refinements(crate::refinements::decode_refinements(section)?);
    }
    // A blob without the overlays section (one predating this format
    // revision, or a compilation whose refinements overlay is empty):
    // empty overlay, not an error — the section is optional on read,
    // mirroring the signatures section's own tolerance rule.
    Ok(index)
}

fn decode_symbol_table(bytes: &[u8]) -> Result<Vec<StubSymbol>, StubBlobError> {
    let mut reader = Reader::new(bytes);
    let count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut symbols = Vec::new();
    for _ in 0..count {
        let kind = reader
            .u8()
            .and_then(StubSymbolKind::from_u8)
            .ok_or(StubBlobError::MalformedSection)?;
        let availability = reader
            .availability()
            .ok_or(StubBlobError::MalformedSection)?;
        let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
        symbols.push(StubSymbol {
            name,
            kind,
            availability,
        });
    }
    Ok(symbols)
}

/// The decoded signatures section: functions and classes, in the
/// shapes `StubIndex::new` accepts directly.
type DecodedSignatures = (
    Vec<(String, StubSignature)>,
    Vec<(String, StubClassSurface)>,
);

/// Decodes the signatures section into `(functions, classes)`,
/// mirroring `encode_signatures`.
fn decode_signatures(bytes: &[u8]) -> Result<DecodedSignatures, StubBlobError> {
    let mut reader = Reader::new(bytes);
    let function_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut functions = Vec::new();
    for _ in 0..function_count {
        let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
        let signature = reader
            .stub_signature()
            .ok_or(StubBlobError::MalformedSection)?;
        functions.push((name, signature));
    }
    let class_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
    let mut classes = Vec::new();
    for _ in 0..class_count {
        let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
        let parent_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
        let mut parents = Vec::new();
        for _ in 0..parent_count {
            parents.push(reader.string().ok_or(StubBlobError::MalformedSection)?);
        }
        let member_count = reader.u32().ok_or(StubBlobError::MalformedSection)?;
        let mut members = Vec::new();
        for _ in 0..member_count {
            let kind = reader
                .u8()
                .and_then(StubMemberKind::from_u8)
                .ok_or(StubBlobError::MalformedSection)?;
            let flags = reader.u8().ok_or(StubBlobError::MalformedSection)?;
            let is_static = flags & 1 != 0;
            let visibility = StubVisibility::from_u8((flags >> 1) & 0b11)
                .ok_or(StubBlobError::MalformedSection)?;
            let availability = reader
                .availability()
                .ok_or(StubBlobError::MalformedSection)?;
            let name = reader.string().ok_or(StubBlobError::MalformedSection)?;
            let type_text = reader
                .versioned_text()
                .ok_or(StubBlobError::MalformedSection)?;
            let has_value = reader.u8().ok_or(StubBlobError::MalformedSection)?;
            let value_text = match has_value {
                0 => None,
                1 => Some(reader.string().ok_or(StubBlobError::MalformedSection)?),
                _ => return Err(StubBlobError::MalformedSection),
            };
            // A method's signature always decodes to `Some(...)`: the
            // writer encodes `None` as an empty signature, so the
            // round trip is not bit-for-bit for that case, only
            // semantically equivalent (an empty signature either way).
            let signature = if kind == StubMemberKind::Method {
                Some(
                    reader
                        .stub_signature()
                        .ok_or(StubBlobError::MalformedSection)?,
                )
            } else {
                None
            };
            members.push(StubMember {
                kind,
                name,
                visibility,
                is_static,
                availability,
                signature,
                type_text,
                value_text,
            });
        }
        classes.push((name, StubClassSurface { parents, members }));
    }
    Ok((functions, classes))
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
pub(crate) struct Reader<'blob> {
    bytes: &'blob [u8],
}

impl<'blob> Reader<'blob> {
    pub(crate) fn new(bytes: &'blob [u8]) -> Self {
        Self { bytes }
    }

    fn take(&mut self, count: usize) -> Option<&'blob [u8]> {
        let (head, tail) = self.bytes.split_at_checked(count)?;
        self.bytes = tail;
        Some(head)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn version(&mut self) -> Option<PhpVersion> {
        let bytes = self.take(2)?;
        Some(PhpVersion::new(*bytes.first()?, *bytes.get(1)?))
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    pub(crate) fn string(&mut self) -> Option<String> {
        let length = usize::try_from(self.u32()?).ok()?;
        let bytes = self.take(length)?;
        core::str::from_utf8(bytes).ok().map(str::to_owned)
    }

    /// Mirrors `write_availability`: `bit0` introduced, `bit1` removed,
    /// `bit2` deprecated, `bit3` since.
    fn availability(&mut self) -> Option<StubAvailability> {
        let flags = self.u8()?;
        let introduced = if flags & 1 != 0 {
            Some(self.version()?)
        } else {
            None
        };
        let removed = if flags & (1 << 1) != 0 {
            Some(self.version()?)
        } else {
            None
        };
        let since = if flags & (1 << 3) != 0 {
            Some(self.version()?)
        } else {
            None
        };
        let deprecated = (flags & (1 << 2) != 0).then_some(StubDeprecation { since });
        Some(StubAvailability {
            introduced,
            removed,
            deprecated,
        })
    }

    fn versioned_text(&mut self) -> Option<VersionedTypeText> {
        let has_default = self.u8()?;
        let default = match has_default {
            0 => None,
            1 => Some(self.string()?),
            _ => return None,
        };
        let override_count = self.u16()?;
        let mut overrides = Vec::new();
        for _ in 0..override_count {
            let version = self.version()?;
            let text = self.string()?;
            overrides.push((version, text));
        }
        Some(VersionedTypeText { default, overrides })
    }

    fn stub_signature(&mut self) -> Option<StubSignature> {
        let by_reference = self.u8()? != 0;
        let return_type = self.versioned_text()?;
        let parameter_count = self.u32()?;
        let mut parameters = Vec::new();
        for _ in 0..parameter_count {
            let flags = self.u8()?;
            let optional = flags & 1 != 0;
            let by_reference = flags & (1 << 1) != 0;
            let variadic = flags & (1 << 2) != 0;
            let availability = self.availability()?;
            let name = self.string()?;
            let type_text = self.versioned_text()?;
            parameters.push(StubParameter {
                name,
                type_text,
                optional,
                by_reference,
                variadic,
                availability,
            });
        }
        Some(StubSignature {
            parameters,
            return_type,
            by_reference,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_project::PhpVersion;

    use super::{
        BLOB_FORMAT_VERSION, BLOB_MAGIC, SECTION_SIGNATURES, SECTION_SYMBOL_TABLE, StubBlobError,
        decode, encode, encode_signatures, encode_symbol_table, fnv1a64,
    };
    use crate::index::StubIndex;
    use crate::signature::{
        StubClassSurface, StubMember, StubMemberKind, StubParameter, StubSignature, StubVisibility,
        VersionedTypeText,
    };
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
        assert_eq!(blob[8..12], 2u32.to_le_bytes());
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
        // Hand-build a version-2 blob whose table carries an unknown
        // section before the symbol table.
        let symbol_table = {
            let encoded = encode(&sample_index());
            // The symbol table of a freshly encoded blob starts right
            // after the header (24) plus three 20-byte table entries
            // (symbol table + signatures + overlays) and runs for
            // exactly `encode_symbol_table`'s own length.
            let symbol_offset = 24 + 3 * 20;
            let symbol_length = encode_symbol_table(&sample_index()).len();
            encoded[symbol_offset..symbol_offset + symbol_length].to_vec()
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

    fn sample_index_with_signatures() -> StubIndex {
        let strlen = StubSignature {
            parameters: vec![StubParameter {
                name: "string".to_owned(),
                type_text: VersionedTypeText::from_text(Some("string".to_owned())),
                optional: false,
                by_reference: false,
                variadic: false,
                availability: StubAvailability::ALWAYS,
            }],
            return_type: VersionedTypeText {
                default: Some("int".to_owned()),
                overrides: vec![(PhpVersion::new(8, 0), "int|false".to_owned())],
            },
            by_reference: false,
        };
        let exception = StubClassSurface {
            parents: vec!["Throwable".to_owned()],
            members: vec![
                StubMember {
                    kind: StubMemberKind::Method,
                    name: "getMessage".to_owned(),
                    visibility: StubVisibility::Public,
                    is_static: false,
                    availability: StubAvailability::ALWAYS,
                    signature: Some(StubSignature {
                        parameters: vec![],
                        return_type: VersionedTypeText::from_text(Some("string".to_owned())),
                        by_reference: false,
                    }),
                    type_text: VersionedTypeText::default(),
                    value_text: None,
                },
                StubMember {
                    kind: StubMemberKind::Property,
                    name: "message".to_owned(),
                    visibility: StubVisibility::Protected,
                    is_static: false,
                    availability: StubAvailability::ALWAYS,
                    signature: None,
                    type_text: VersionedTypeText::from_text(Some("string".to_owned())),
                    value_text: None,
                },
            ],
        };
        StubIndex::new(
            sample_index().symbols().to_vec(),
            vec![("strlen".to_owned(), strlen)],
            vec![("Exception".to_owned(), exception)],
        )
    }

    #[test]
    fn signatures_round_trip_through_the_blob() {
        let index = sample_index_with_signatures();
        assert_eq!(decode(&encode(&index)), Ok(index));
    }

    #[test]
    fn a_blob_without_the_signature_section_decodes_with_empty_payloads() {
        // The pre-plan-3 encoding: hand-build a genuinely one-section
        // blob (magic + current version + checksum patch + a single
        // symbol-table entry), mirroring
        // `unknown_sections_are_skipped_for_forward_compatibility`'s
        // construction. `encode` always writes the signatures section
        // now, so this is the only way left to exercise a blob that
        // never had one.
        let old_index = sample_index();
        let symbol_table = encode_symbol_table(&old_index);
        let table_entries = 1u32;
        let symbol_offset = 24u64 + u64::from(table_entries) * 20;
        let mut blob = Vec::new();
        blob.extend_from_slice(&BLOB_MAGIC);
        blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
        blob.extend_from_slice(&[0; 8]);
        blob.extend_from_slice(&table_entries.to_le_bytes());
        blob.extend_from_slice(&SECTION_SYMBOL_TABLE.to_le_bytes());
        blob.extend_from_slice(&symbol_offset.to_le_bytes());
        blob.extend_from_slice(&(symbol_table.len() as u64).to_le_bytes());
        blob.extend_from_slice(&symbol_table);
        let checksum = fnv1a64(&blob[20..]);
        blob[12..20].copy_from_slice(&checksum.to_le_bytes());
        let decoded = decode(&blob).unwrap();
        assert_eq!(decoded.symbols(), old_index.symbols());
        assert!(decoded.functions().is_empty());
        assert!(decoded.classes().is_empty());
    }

    #[test]
    fn the_format_version_is_two_and_the_table_carries_three_sections() {
        let blob = encode(&sample_index());
        assert_eq!(blob.get(8..12), Some(2u32.to_le_bytes().as_slice()));
        assert_eq!(blob.get(20..24), Some(3u32.to_le_bytes().as_slice()));
    }

    #[test]
    fn refinements_round_trip_through_the_blob() {
        let mut index = sample_index();
        index.set_refinements(crate::refinements::StubRefinements::new(
            vec![(
                "array_keys".to_owned(),
                crate::refinements::RefinedSignature {
                    templates: vec![],
                    parameters: vec![],
                    return_type: Some("list<int>".to_owned()),
                },
            )],
            vec![(
                "arrayiterator".to_owned(),
                crate::refinements::RefinedClass {
                    templates: vec![
                        crate::refinements::RefinedTemplate {
                            name: "TKey".to_owned(),
                            bound: None,
                        },
                        crate::refinements::RefinedTemplate {
                            name: "TValue".to_owned(),
                            bound: Some("object".to_owned()),
                        },
                    ],
                    ancestors: vec![crate::refinements::RefinedAncestor {
                        name: "iterator".to_owned(),
                        arguments: vec!["TKey".to_owned(), "TValue".to_owned()],
                    }],
                    methods: vec![
                        (
                            "current".to_owned(),
                            crate::refinements::RefinedSignature {
                                templates: vec![],
                                parameters: vec![],
                                return_type: Some("TValue".to_owned()),
                            },
                        ),
                        (
                            "key".to_owned(),
                            crate::refinements::RefinedSignature {
                                templates: vec![],
                                parameters: vec![],
                                return_type: Some("TKey".to_owned()),
                            },
                        ),
                    ],
                },
            )],
        ));
        assert_eq!(decode(&encode(&index)), Ok(index));
    }

    #[test]
    fn a_blob_without_the_overlays_section_decodes_with_empty_refinements() {
        // Build a two-section, version-2 blob by hand: magic + version
        // 2 + checksum patch + a symbol-table entry + a signatures
        // entry, but no overlays entry. This is the exact tolerance
        // rule decision 4 relies on: the signatures section already
        // has it (see
        // `a_blob_without_the_signature_section_decodes_with_empty_payloads`),
        // and the overlays section must have it too.
        let old_index = sample_index();
        let symbol_table = encode_symbol_table(&old_index);
        let signatures = encode_signatures(&old_index);
        let table_entries = 2u32;
        let symbol_offset = 24u64 + u64::from(table_entries) * 20;
        let signature_offset = symbol_offset + symbol_table.len() as u64;
        let mut blob = Vec::new();
        blob.extend_from_slice(&BLOB_MAGIC);
        blob.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
        blob.extend_from_slice(&[0; 8]);
        blob.extend_from_slice(&table_entries.to_le_bytes());
        blob.extend_from_slice(&SECTION_SYMBOL_TABLE.to_le_bytes());
        blob.extend_from_slice(&symbol_offset.to_le_bytes());
        blob.extend_from_slice(&(symbol_table.len() as u64).to_le_bytes());
        blob.extend_from_slice(&SECTION_SIGNATURES.to_le_bytes());
        blob.extend_from_slice(&signature_offset.to_le_bytes());
        blob.extend_from_slice(&(signatures.len() as u64).to_le_bytes());
        blob.extend_from_slice(&symbol_table);
        blob.extend_from_slice(&signatures);
        let checksum = fnv1a64(&blob[20..]);
        blob[12..20].copy_from_slice(&checksum.to_le_bytes());
        let decoded = decode(&blob).unwrap();
        assert!(decoded.refinements().is_empty());
    }

    #[test]
    fn a_truncated_signature_section_never_panics() {
        let blob = encode(&sample_index_with_signatures());
        for length in 0..blob.len() {
            // Every prefix is an error or a clean decode, never a panic.
            let _ = decode(&blob[..length]);
        }
    }

    #[test]
    fn a_malformed_signature_section_is_a_clean_rejection() {
        let mut blob = encode(&sample_index_with_signatures());
        // Flip a byte deep in the payload (past the header and table),
        // then re-patch the checksum so decoding reaches the section.
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        let checksum = fnv1a64(&blob[20..]);
        blob[12..20].copy_from_slice(&checksum.to_le_bytes());
        // Either a clean error or a decode that differs — never a panic.
        let _ = decode(&blob);
    }
}
