use text_size::{TextRange, TextSize};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// The decoded text would exceed the 4 GiB cap of [`TextSize`].
///
/// This is the only way decoding fails; the caller renders it as a
/// diagnostic. Everything else — invalid bytes, a byte-order mark — is
/// provenance data on the decoded [`SourceText`], not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceTooLarge {
    /// Byte length the decoded text would have reached.
    pub decoded_length: usize,
}

impl core::fmt::Display for SourceTooLarge {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "decoded source text would be {} bytes, beyond the 4 GiB maximum",
            self.decoded_length
        )
    }
}

impl std::error::Error for SourceTooLarge {}

/// Source bytes decoded into engine-ready UTF-8 text, with provenance.
///
/// Decoding strips a leading UTF-8 byte-order mark (recorded in
/// [`had_utf8_bom`](Self::had_utf8_bom)) and replaces invalid UTF-8
/// sequences with U+FFFD (each replacement's range in the decoded text is
/// recorded in [`replacements`](Self::replacements)). No other
/// normalization happens: line endings, tabs, and NUL bytes pass through
/// untouched, and the lossless syntax-tree guarantee is measured against
/// this decoded text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceText {
    text: String,
    had_utf8_bom: bool,
    replacements: Vec<TextRange>,
}

impl SourceText {
    /// Decodes raw file bytes. The only failure is [`SourceTooLarge`];
    /// every byte sequence otherwise decodes to a usable text.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SourceTooLarge> {
        let (had_utf8_bom, content) = match bytes.strip_prefix(UTF8_BOM) {
            Some(rest) => (true, rest),
            None => (false, bytes),
        };
        // The decoded text is never shorter than the input (valid bytes
        // copy one to one; invalid sequences of at most three bytes become
        // a three-byte U+FFFD), so oversized inputs fail before decoding.
        text_size_of(content.len())?;
        let (text, replacements) = decode_lossy(content)?;
        text_size_of(text.len())?;
        Ok(Self {
            text,
            had_utf8_bom,
            replacements,
        })
    }

    /// The decoded UTF-8 text, byte-order mark stripped.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether a leading UTF-8 byte-order mark was stripped. Writers can
    /// re-emit it; a byte-order mark before `<?php` is also a PHP hazard
    /// (bytes before the opening tag are sent to output) worth a future
    /// lint.
    pub fn had_utf8_bom(&self) -> bool {
        self.had_utf8_bom
    }

    /// Ranges in [`text`](Self::text) where invalid bytes were replaced
    /// with U+FFFD. Distinguishes real corruption from a literal U+FFFD
    /// present in the file.
    pub fn replacements(&self) -> &[TextRange] {
        &self.replacements
    }

    /// True when the decoded text is byte-for-byte the input: no
    /// byte-order mark, no replacements. Upper layers must consult this
    /// before writing autofixes back to disk.
    pub fn is_pristine(&self) -> bool {
        !self.had_utf8_bom && self.replacements.is_empty()
    }
}

/// Converts a byte length within the decoded text into a [`TextSize`],
/// rejecting lengths beyond the 4 GiB cap.
fn text_size_of(length: usize) -> Result<TextSize, SourceTooLarge> {
    u32::try_from(length)
        .map(TextSize::from)
        .map_err(|_| SourceTooLarge {
            decoded_length: length,
        })
}

/// Decodes bytes to UTF-8, replacing invalid sequences with U+FFFD.
/// Replacement-range tracking arrives with the invalid-input tests; for
/// valid input the lossy conversion is a borrowed pass-through.
fn decode_lossy(bytes: &[u8]) -> Result<(String, Vec<TextRange>), SourceTooLarge> {
    Ok((String::from_utf8_lossy(bytes).into_owned(), Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::{SourceTooLarge, text_size_of};

    #[test]
    fn lengths_within_the_cap_convert() {
        assert!(text_size_of(0).is_ok());
        assert!(text_size_of(u32::MAX as usize).is_ok());
    }

    #[test]
    fn lengths_beyond_the_cap_are_rejected() {
        let length = u32::MAX as usize + 1;
        assert_eq!(
            text_size_of(length),
            Err(SourceTooLarge {
                decoded_length: length
            })
        );
    }
}
