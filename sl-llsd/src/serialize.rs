//! Self-describing LLSD payloads: the `<? … ?>` header line that names an
//! encoding, and the permissive reader that honours it.
//!
//! The three codecs beside this one ([`parse_llsd_xml`], [`parse_llsd_binary`],
//! [`parse_llsd_notation`]) each decode one encoding, and every caller that
//! already knows which one it is holding goes straight to them. Some payloads
//! do **not** say which they are in their surrounding protocol: an EEP settings
//! asset, a GLTF material-override document, and the `PushExpEnvironment`
//! generic message all carry "an LLSD document" and leave the encoding to the
//! sender. The reference resolves those with `LLSDSerialize::serialize` /
//! `LLSDSerialize::deserialize` (`indra/llcommon/llsdserialize.cpp`), which
//! write and read an optional header line naming the encoding; this module is
//! that pair.
//!
//! Sniffing alone cannot stand in for the header: a notation map opens with
//! `{`, which is also binary LLSD's map marker, so a headerless payload is
//! genuinely ambiguous and the reference's own fallback order
//! (`<` ⇒ XML, otherwise notation) is what a viewer in the wild will apply.

use crate::error::LlsdError;
use crate::value::{Llsd, parse_llsd_xml};
use crate::{parse_llsd_binary, parse_llsd_notation};

/// Which encoding a serialized LLSD document is written in — the three
/// `LLSDSerialize::ELLSD_Serialize` alternatives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlsdEncoding {
    /// LLSD-XML (`<? LLSD/XML ?>`).
    Xml,
    /// Binary LLSD (`<? LLSD/Binary ?>`).
    Binary,
    /// Notation LLSD (`<? llsd/notation ?>`) — what the reference writes a
    /// settings asset in.
    Notation,
}

impl LlsdEncoding {
    /// The encoding's name as it appears between the `<?` and `?>` of a header
    /// line, spelled exactly as the reference writes it (`LLSD/Binary`,
    /// `LLSD/XML`, `llsd/notation` — the inconsistent casing is the
    /// reference's, and it compares case-insensitively when reading).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Xml => "LLSD/XML",
            Self::Binary => "LLSD/Binary",
            Self::Notation => "llsd/notation",
        }
    }

    /// The complete header line this encoding is announced by, newline
    /// included.
    #[must_use]
    pub const fn header(self) -> &'static str {
        match self {
            Self::Xml => "<? LLSD/XML ?>\n",
            Self::Binary => "<? LLSD/Binary ?>\n",
            Self::Notation => "<? llsd/notation ?>\n",
        }
    }

    /// The encoding a header line names, or `None` for a line naming none —
    /// matched case-insensitively, as `LLSDSerialize::deserialize` does.
    ///
    /// `line` is the header **without** its `<?`/`?>` delimiters or trailing
    /// newline; [`split_header_line`] hands over exactly that.
    #[must_use]
    fn from_header_name(line: &str) -> Option<Self> {
        [Self::Xml, Self::Binary, Self::Notation]
            .into_iter()
            .find(|encoding| line.eq_ignore_ascii_case(encoding.name()))
    }

    /// Decode `bytes` in this encoding.
    fn parse(self, bytes: &[u8]) -> Result<Llsd, LlsdError> {
        match self {
            Self::Xml => std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| parse_llsd_xml(text).ok())
                .ok_or(LlsdError::MalformedXml),
            Self::Binary => parse_llsd_binary(bytes),
            Self::Notation => parse_llsd_notation(bytes),
        }
    }
}

/// Serializes `value` as a self-describing LLSD document: the `<? … ?>` header
/// line naming `encoding`, then the body — the reference's
/// `LLSDSerialize::serialize`.
///
/// The header is not decoration. A payload written without one is read back by
/// guesswork (see [`parse_llsd_serialized`]), and the two textual encodings are
/// not reliably distinguishable by sight, so anything this workspace *emits*
/// says what it is.
#[must_use]
pub fn to_llsd_serialized(value: &Llsd, encoding: LlsdEncoding) -> Vec<u8> {
    let mut bytes = encoding.header().as_bytes().to_vec();
    match encoding {
        LlsdEncoding::Xml => bytes.extend_from_slice(value.to_llsd_xml().as_bytes()),
        LlsdEncoding::Binary => bytes.extend_from_slice(&value.to_llsd_binary()),
        LlsdEncoding::Notation => bytes.extend_from_slice(&value.to_llsd_notation()),
    }
    bytes
}

/// Decodes a serialized LLSD document in whichever of the three encodings it
/// turns out to be — the reference's `LLSDSerialize::deserialize`.
///
/// A `<? … ?>` header line naming an encoding settles it. Failing that, a
/// document opening `<llsd` or `<?xml` is XML (the reference's
/// `LEGACY_NON_HEADER` case), and anything else is tried as binary and then as
/// notation — the reference assumes notation outright there, but binary is
/// cheap to reject and a headerless binary body is a real thing to receive.
///
/// # Errors
///
/// Returns the last codec's error when the payload decodes as none of them.
pub fn parse_llsd_serialized(bytes: &[u8]) -> Result<Llsd, LlsdError> {
    let (header, payload) = split_header_line(bytes);
    if let Some(encoding) = header
        .and_then(|line| std::str::from_utf8(line).ok())
        .and_then(LlsdEncoding::from_header_name)
    {
        return encoding.parse(payload);
    }
    // No header we recognize. XML announces itself with its own opening; the
    // rest is a coin toss the reference calls "notation".
    if looks_like_xml(payload) {
        return LlsdEncoding::Xml.parse(payload);
    }
    if let Ok(value) = parse_llsd_binary(payload) {
        return Ok(value);
    }
    parse_llsd_notation(payload)
}

/// Whether `bytes` opens the way an LLSD-XML document does — the reference's
/// `LEGACY_NON_HEADER` (`<llsd>`) check, widened to the `<?xml` prolog a
/// conforming document may carry first.
fn looks_like_xml(bytes: &[u8]) -> bool {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .and_then(|first| bytes.get(first..))
        .unwrap_or_default();
    start.starts_with(b"<llsd") || start.starts_with(b"<?xml")
}

/// Splits `bytes` at a leading `<? … ?>` LLSD header line into that line's
/// **name** (the text between the delimiters, trimmed) and the payload after
/// it, or `(None, bytes)` when there is none.
///
/// An `<?xml` prolog is deliberately not one: it belongs to the XML document
/// the caller parses whole, not to an LLSD header.
fn split_header_line(bytes: &[u8]) -> (Option<&[u8]>, &[u8]) {
    if !bytes.starts_with(b"<?") || bytes.starts_with(b"<?xml") {
        return (None, bytes);
    }
    let Some(newline) = bytes.iter().position(|&byte| byte == b'\n') else {
        return (None, bytes);
    };
    let (Some(line), Some(payload)) =
        (bytes.get(..newline), bytes.get(newline.saturating_add(1)..))
    else {
        return (None, bytes);
    };
    let name = trim_ascii(line.strip_prefix(b"<?").unwrap_or(line));
    let name = trim_ascii(name.strip_suffix(b"?>").unwrap_or(name));
    (Some(name), payload)
}

/// `bytes` with leading and trailing ASCII whitespace removed.
const fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let mut slice = bytes;
    while let Some((first, rest)) = slice.split_first() {
        if !first.is_ascii_whitespace() {
            break;
        }
        slice = rest;
    }
    while let Some((last, rest)) = slice.split_last() {
        if !last.is_ascii_whitespace() {
            break;
        }
        slice = rest;
    }
    slice
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;

    use super::{LlsdEncoding, LlsdError, parse_llsd_serialized, to_llsd_serialized};
    use crate::Llsd;

    /// A small map exercising every scalar kind a push payload carries.
    fn sample() -> Llsd {
        Llsd::Map(HashMap::from([
            (
                "action".to_owned(),
                Llsd::String("PushFullEnvironment".to_owned()),
            ),
            ("transition_time".to_owned(), Llsd::Real(4.0)),
            ("count".to_owned(), Llsd::Integer(1)),
        ]))
    }

    #[test]
    fn every_encoding_round_trips_through_its_own_header() {
        for encoding in [
            LlsdEncoding::Xml,
            LlsdEncoding::Binary,
            LlsdEncoding::Notation,
        ] {
            let bytes = to_llsd_serialized(&sample(), encoding);
            assert!(
                bytes.starts_with(encoding.header().as_bytes()),
                "{encoding:?} did not write its own header"
            );
            assert_eq!(
                parse_llsd_serialized(&bytes),
                Ok(sample()),
                "{encoding:?} did not round-trip"
            );
        }
    }

    #[test]
    fn a_header_is_matched_case_insensitively() {
        let body = sample().to_llsd_notation();
        let mut bytes = b"<? LLSD/NOTATION ?>\n".to_vec();
        bytes.extend_from_slice(&body);
        assert_eq!(parse_llsd_serialized(&bytes), Ok(sample()));
    }

    #[test]
    fn a_headerless_notation_body_is_read_as_notation() {
        let bytes = sample().to_llsd_notation();
        assert_eq!(parse_llsd_serialized(&bytes), Ok(sample()));
    }

    #[test]
    fn a_headerless_xml_body_is_read_as_xml() {
        let bytes = sample().to_llsd_xml().into_bytes();
        assert_eq!(parse_llsd_serialized(&bytes), Ok(sample()));
    }

    #[test]
    fn a_headerless_binary_body_is_read_as_binary() {
        let bytes = sample().to_llsd_binary();
        assert_eq!(parse_llsd_serialized(&bytes), Ok(sample()));
    }

    #[test]
    fn an_xml_prolog_is_not_taken_for_an_llsd_header() {
        let mut text = String::from("<?xml version=\"1.0\" ?>\n");
        text.push_str(&sample().to_llsd_xml());
        assert_eq!(parse_llsd_serialized(text.as_bytes()), Ok(sample()));
    }

    #[test]
    fn a_body_that_decodes_as_nothing_is_an_error() {
        assert!(matches!(
            parse_llsd_serialized(b"\x01not llsd at all"),
            Err(LlsdError::MalformedNotation | LlsdError::TruncatedBinary)
        ));
    }
}
