//! Codec boundary for Second Life URL wire fields.
//!
//! Many wire and LLSD/capability fields carry URLs as raw strings (the grid's
//! map/search endpoints, a parcel's media/music stream, an object's media URL, a
//! capability seed URL, …). A conforming peer either sends a parseable URL or the
//! *empty* string, which is the "absent / not set" sentinel. These helpers wrap
//! that convention so the typed surface can hold a [`url::Url`] (or
//! [`Option<url::Url>`], `None` for the empty sentinel) while a non-empty but
//! unparsable value is rejected rather than silently kept as an invalid string —
//! the same non-masking stance as [`region_name_from_wire`](crate::region_name)
//! and the UUID/scalar boundary helpers.

use url::Url;

use crate::error::WireError;

/// Decode a raw wire string into a required [`Url`].
///
/// The value is parsed through [`Url::parse`]; an empty value or one that fails
/// to parse is rejected with [`WireError::InvalidUrl`] rather than masked, so a
/// malformed message is dropped (and surfaced as a diagnostic) instead of
/// masquerading as a valid URL. Use [`optional_url_from_wire`] for fields whose
/// empty value is a legitimate "absent" sentinel. The inverse on encode is
/// [`url_to_wire`].
///
/// # Errors
///
/// Returns [`WireError::InvalidUrl`] when `raw` is empty or does not parse as a
/// URL.
pub fn url_from_wire(field: &'static str, raw: &str) -> Result<Url, WireError> {
    Url::parse(raw).map_err(|_invalid| WireError::InvalidUrl {
        field,
        value: raw.to_owned(),
    })
}

/// Decode a raw wire string into an [`Option<Url>`].
///
/// An empty (or whitespace-only) value is the "absent" sentinel and decodes to
/// `None`. A non-empty value is parsed through [`Url::parse`]; a value that fails
/// to parse is rejected with [`WireError::InvalidUrl`] rather than masked. The
/// inverse on encode is [`optional_url_to_wire`].
///
/// # Errors
///
/// Returns [`WireError::InvalidUrl`] when `raw` is non-empty but does not parse
/// as a URL.
pub fn optional_url_from_wire(field: &'static str, raw: &str) -> Result<Option<Url>, WireError> {
    if raw.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(url_from_wire(field, raw)?))
    }
}

/// Encode a [`Url`] back into its raw wire string (its canonical serialization).
/// The inverse of [`url_from_wire`].
#[must_use]
pub fn url_to_wire(url: &Url) -> String {
    url.as_str().to_owned()
}

/// Encode an [`Option<Url>`] back into its raw wire string: the URL when `Some`,
/// or the empty "absent" sentinel when `None`. The inverse of
/// [`optional_url_from_wire`].
#[must_use]
pub fn optional_url_to_wire(url: Option<&Url>) -> String {
    match url {
        Some(value) => value.as_str().to_owned(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Capability query strings
//
// Several capabilities are plain GETs whose arguments ride in the URL's query
// string (`FindExperienceByName`, `AvatarPickerSearch`, the AIS3 fetches, …).
// These are the shared pieces both directions need: a client builds the suffix,
// a simulator parses it back.
// ---------------------------------------------------------------------------

/// Percent-encodes `text` for a URL query value: the RFC 3986 unreserved set is
/// kept verbatim, every other byte becomes `%XX`. The inverse is
/// [`percent_decode`].
pub(crate) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(byte & 0x0f));
        }
    }
    out
}

/// Maps a nibble (0–15) to its uppercase ASCII hex digit (a match, so no
/// arithmetic or indexing).
const fn hex_digit(nibble: u8) -> char {
    match nibble {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'A',
        11 => 'B',
        12 => 'C',
        13 => 'D',
        14 => 'E',
        _ => 'F',
    }
}

/// Maps an ASCII hex digit (`0-9`, `a-f`, `A-F`) to its nibble value, or `None`.
const fn from_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(byte.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(byte.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

/// Decodes a percent-encoded URL query value — the inverse of
/// [`percent_encode`]. A `%XX` pair becomes its byte; a malformed `%` (not
/// followed by two hex digits) is kept verbatim. The resulting bytes are
/// interpreted as UTF-8 (lossily, since the encoder only ever emits valid
/// UTF-8).
pub(crate) fn percent_decode(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut iter = text.bytes();
    while let Some(byte) = iter.next() {
        if byte == b'%' {
            let high = iter.next();
            let low = iter.next();
            match (high.and_then(from_hex_digit), low.and_then(from_hex_digit)) {
                (Some(high), Some(low)) => bytes.push(high.wrapping_shl(4) | low),
                _ => {
                    bytes.push(b'%');
                    if let Some(high) = high {
                        bytes.push(high);
                    }
                    if let Some(low) = low {
                        bytes.push(low);
                    }
                }
            }
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Returns the query string of a `{cap}{suffix}` URL — everything after the
/// first `?` — or `None` when the suffix carries no query.
pub(crate) fn url_query(suffix: &str) -> Option<&str> {
    suffix.split_once('?').map(|(_path, query)| query)
}

/// Returns the value of query parameter `name` within a `key=value&…` query
/// string, if present.
pub(crate) fn query_param<'query>(query: &'query str, name: &str) -> Option<&'query str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

#[cfg(test)]
mod test {
    use super::{optional_url_from_wire, optional_url_to_wire, url_from_wire, url_to_wire};
    use pretty_assertions::assert_eq;

    #[test]
    fn empty_decodes_to_none_and_round_trips() -> Result<(), crate::error::WireError> {
        assert_eq!(optional_url_from_wire("MediaURL", "")?, None);
        assert_eq!(optional_url_from_wire("MediaURL", "   ")?, None);
        assert_eq!(optional_url_to_wire(None), "");
        Ok(())
    }

    #[test]
    fn valid_url_round_trips_bit_identically() -> Result<(), crate::error::WireError> {
        // A canonical URL survives a wire round-trip byte-for-byte.
        let raw = "http://stream.example.com:8000/live";
        let decoded = url_from_wire("MusicURL", raw)?;
        assert_eq!(url_to_wire(&decoded), raw);

        let optional = optional_url_from_wire("MusicURL", raw)?;
        assert!(optional.is_some());
        assert_eq!(optional_url_to_wire(optional.as_ref()), raw);
        Ok(())
    }

    #[test]
    fn slurl_scheme_round_trips() -> Result<(), crate::error::WireError> {
        // A SLURL uses the `secondlife` scheme, which `url` parses fine.
        let raw = "secondlife:///app/agent/00000000-0000-0000-0000-000000000000/about";
        let decoded = url_from_wire("slurl", raw)?;
        assert_eq!(url_to_wire(&decoded), raw);
        Ok(())
    }

    #[test]
    fn non_empty_invalid_url_is_rejected() {
        assert_eq!(
            url_from_wire("MediaURL", "not a url"),
            Err(crate::error::WireError::InvalidUrl {
                field: "MediaURL",
                value: "not a url".to_owned(),
            })
        );
        // Empty is a hard error for the required form, the "absent" sentinel for
        // the optional one.
        assert_eq!(
            url_from_wire("MediaURL", ""),
            Err(crate::error::WireError::InvalidUrl {
                field: "MediaURL",
                value: String::new(),
            })
        );
        assert_eq!(
            optional_url_from_wire("MediaURL", "http://["),
            Err(crate::error::WireError::InvalidUrl {
                field: "MediaURL",
                value: "http://[".to_owned(),
            })
        );
    }
}
