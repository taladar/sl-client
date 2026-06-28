//! The binary-LLSD codec — the byte-compatible serialization Second Life
//! viewers use for the on-disk inventory cache.
//!
//! [`Llsd::to_llsd_binary`] and [`parse_llsd_binary`] implement the same wire
//! shape as Firestorm's `LLSDBinaryFormatter` / `LLSDBinaryParser`
//! (`indra/llcommon/llsdserialize.cpp`), so a cache file written here is
//! readable by the reference viewer and vice-versa. Each value is a one-byte
//! type marker followed by its payload; multi-byte integers and reals are
//! big-endian (network byte order):
//!
//! | Marker | Kind | Payload |
//! |---|---|---|
//! | `!` | undef | — |
//! | `1` / `0` | boolean | — (the marker *is* the value) |
//! | `i` | integer | 4-byte big-endian `i32` |
//! | `r` | real | 8-byte big-endian `f64` |
//! | `u` | uuid | 16 raw bytes |
//! | `s` | string | 4-byte big-endian length + UTF-8 bytes |
//! | `l` | uri | 4-byte big-endian length + UTF-8 bytes |
//! | `d` | date | 8-byte *host-endian* `f64` epoch-seconds |
//! | `b` | binary | 4-byte big-endian length + raw bytes |
//! | `[` | array | 4-byte big-endian count + values + `]` |
//! | `{` | map | 4-byte big-endian count + (`k` + key string + value)\* + `}` |
//!
//! Two Firestorm-pinned wrinkles this codec honours:
//!
//! - The closing `]` / `}` are **mandatory**: `LLSDBinaryParser::parseArray` /
//!   `parseMap` return `PARSE_FAILURE` if the terminator is absent. The 4-byte
//!   count is authoritative — exactly that many entries are read, then the
//!   terminator is required.
//! - The `date` payload is written *raw* (host-endian) by Firestorm's
//!   `format_impl`, unlike `real` which goes through `ll_htond` (big-endian); so
//!   this codec writes/reads `Date` host-endian to stay byte-compatible. The
//!   inventory cache never exercises this path — item creation dates serialise
//!   as LLSD `Integer`, not `Date` — but the general round-trip honours it.
//!
//! On read the parser also tolerates Firestorm's notation-style `'` / `"`
//! delimited strings (and quoted map keys), but only ever *emits* the
//! length-prefixed `s` / `k` forms.

use std::collections::HashMap;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::error::LlsdError;
use crate::value::Llsd;

/// The undefined-value marker (`!`).
const MARKER_UNDEF: u8 = b'!';
/// The boolean-true marker (`1`); the marker itself carries the value.
const MARKER_TRUE: u8 = b'1';
/// The boolean-false marker (`0`); the marker itself carries the value.
const MARKER_FALSE: u8 = b'0';
/// The integer marker (`i`), followed by a 4-byte big-endian `i32`.
const MARKER_INTEGER: u8 = b'i';
/// The real marker (`r`), followed by an 8-byte big-endian `f64`.
const MARKER_REAL: u8 = b'r';
/// The UUID marker (`u`), followed by 16 raw bytes.
const MARKER_UUID: u8 = b'u';
/// The string marker (`s`), followed by a 4-byte length and UTF-8 bytes.
const MARKER_STRING: u8 = b's';
/// The URI marker (`l`), followed by a 4-byte length and UTF-8 bytes.
const MARKER_URI: u8 = b'l';
/// The date marker (`d`), followed by an 8-byte host-endian `f64`.
const MARKER_DATE: u8 = b'd';
/// The binary marker (`b`), followed by a 4-byte length and raw bytes.
const MARKER_BINARY: u8 = b'b';
/// The array-open marker (`[`), followed by a 4-byte count.
const MARKER_ARRAY_BEGIN: u8 = b'[';
/// The mandatory array-close marker (`]`).
const MARKER_ARRAY_END: u8 = b']';
/// The map-open marker (`{`), followed by a 4-byte count.
const MARKER_MAP_BEGIN: u8 = b'{';
/// The mandatory map-close marker (`}`).
const MARKER_MAP_END: u8 = b'}';
/// The map-key marker (`k`), followed by a 4-byte length and UTF-8 bytes.
const MARKER_MAP_KEY: u8 = b'k';
/// The notation-string single-quote delimiter, tolerated on read.
const MARKER_NOTATION_SQUOTE: u8 = b'\'';
/// The notation-string double-quote delimiter, tolerated on read.
const MARKER_NOTATION_DQUOTE: u8 = b'"';

/// Big-endian byte conversions for binary-LLSD scalars.
///
/// The crate-wide `big_endian_bytes` / `little_endian_bytes` clippy lints forbid
/// the bare `to_be_bytes` / `from_be_bytes` calls, so they are confined here
/// behind one localized expectation; the date path uses host-endian
/// `to_ne_bytes` / `from_ne_bytes` directly (matching Firestorm's raw `Date`
/// write), which no lint forbids.
mod big_endian {
    #![expect(
        clippy::big_endian_bytes,
        reason = "binary LLSD multi-byte integers, lengths and reals are wire-defined big-endian (network byte order)"
    )]

    /// Encodes a `u32` length/count as four big-endian bytes.
    pub(super) const fn u32_to(value: u32) -> [u8; 4] {
        value.to_be_bytes()
    }

    /// Decodes a big-endian `u32` length/count from four bytes.
    pub(super) const fn u32_from(bytes: [u8; 4]) -> u32 {
        u32::from_be_bytes(bytes)
    }

    /// Encodes an `i32` integer value as four big-endian bytes.
    pub(super) const fn i32_to(value: i32) -> [u8; 4] {
        value.to_be_bytes()
    }

    /// Decodes a big-endian `i32` integer value from four bytes.
    pub(super) const fn i32_from(bytes: [u8; 4]) -> i32 {
        i32::from_be_bytes(bytes)
    }

    /// Encodes an `f64` real value as eight big-endian bytes.
    pub(super) const fn f64_to(value: f64) -> [u8; 8] {
        value.to_be_bytes()
    }

    /// Decodes a big-endian `f64` real value from eight bytes.
    pub(super) const fn f64_from(bytes: [u8; 8]) -> f64 {
        f64::from_be_bytes(bytes)
    }
}

/// Serializes `value` as binary LLSD — the implementation behind
/// [`Llsd::to_llsd_binary`], kept a free function so binary.rs needs no second
/// inherent `impl Llsd` block (the method itself lives beside `to_llsd_xml`).
pub(crate) fn to_binary(value: &Llsd) -> Vec<u8> {
    let mut out = Vec::new();
    push_binary(value, &mut out);
    out
}

/// Appends `value`'s binary-LLSD encoding to `out`, recursing into arrays and
/// maps. The byte-level inverse of [`parse_value`].
fn push_binary(value: &Llsd, out: &mut Vec<u8>) {
    match value {
        Llsd::Undef => out.push(MARKER_UNDEF),
        Llsd::Boolean(flag) => out.push(if *flag { MARKER_TRUE } else { MARKER_FALSE }),
        Llsd::Integer(integer) => {
            out.push(MARKER_INTEGER);
            out.extend_from_slice(&big_endian::i32_to(*integer));
        }
        Llsd::Real(real) => {
            out.push(MARKER_REAL);
            out.extend_from_slice(&big_endian::f64_to(*real));
        }
        Llsd::Uuid(uuid) => {
            out.push(MARKER_UUID);
            out.extend_from_slice(uuid.as_bytes());
        }
        Llsd::String(string) => push_binary_string(out, MARKER_STRING, string),
        Llsd::Uri(uri) => push_binary_string(out, MARKER_URI, uri),
        Llsd::Date(date) => {
            out.push(MARKER_DATE);
            out.extend_from_slice(&date_string_to_epoch_bytes(date));
        }
        Llsd::Binary(blob) => {
            out.push(MARKER_BINARY);
            out.extend_from_slice(&big_endian::u32_to(len_as_u32(blob.len())));
            out.extend_from_slice(blob);
        }
        Llsd::Array(values) => {
            out.push(MARKER_ARRAY_BEGIN);
            out.extend_from_slice(&big_endian::u32_to(len_as_u32(values.len())));
            for element in values {
                push_binary(element, out);
            }
            out.push(MARKER_ARRAY_END);
        }
        Llsd::Map(map) => {
            out.push(MARKER_MAP_BEGIN);
            out.extend_from_slice(&big_endian::u32_to(len_as_u32(map.len())));
            let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            for (key, member) in entries {
                push_binary_string(out, MARKER_MAP_KEY, key);
                push_binary(member, out);
            }
            out.push(MARKER_MAP_END);
        }
    }
}

/// Appends a length-prefixed string (`marker` + 4-byte big-endian length +
/// UTF-8 bytes) — the encoding shared by strings, URIs and map keys.
fn push_binary_string(out: &mut Vec<u8>, marker: u8, value: &str) {
    out.push(marker);
    out.extend_from_slice(&big_endian::u32_to(len_as_u32(value.len())));
    out.extend_from_slice(value.as_bytes());
}

/// Narrows a `usize` length/count to the `u32` the wire format carries, clamping
/// the (practically unreachable) `> 4 GiB` case to `u32::MAX` rather than
/// panicking.
fn len_as_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// Converts an ISO-8601 date string to the 8 *host-endian* bytes of its `f64`
/// epoch-seconds, matching Firestorm's raw `Date` write. An unparsable string
/// encodes as `0.0` (the epoch) so the infallible writer cannot fail.
fn date_string_to_epoch_bytes(iso: &str) -> [u8; 8] {
    let seconds = OffsetDateTime::parse(iso, &Rfc3339).map_or(0.0, |when| {
        timestamp_to_f64(when.unix_timestamp(), when.nanosecond())
    });
    seconds.to_ne_bytes()
}

/// Combines whole seconds and a sub-second nanosecond count into `f64`
/// epoch-seconds — the lossy double timestamp Firestorm's binary `Date` carries.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "epoch seconds are an f64 in the binary LLSD format; the precision loss matches Firestorm's lossy double timestamp"
)]
fn timestamp_to_f64(seconds: i64, nanos: u32) -> f64 {
    seconds as f64 + f64::from(nanos) / 1_000_000_000.0
}

/// Converts `f64` epoch-seconds back to an ISO-8601 (RFC 3339) date string.
///
/// # Errors
/// Returns [`LlsdError::InvalidBinaryDate`] if the value is not a representable
/// calendar timestamp.
fn epoch_to_date_string(seconds: f64) -> Result<String, LlsdError> {
    let when = OffsetDateTime::from_unix_timestamp_nanos(f64_seconds_to_nanos(seconds))
        .map_err(|_ignored| LlsdError::InvalidBinaryDate)?;
    when.format(&Rfc3339)
        .map_err(|_ignored| LlsdError::InvalidBinaryDate)
}

/// Converts `f64` epoch-seconds to the `i128` epoch-nanoseconds
/// [`OffsetDateTime::from_unix_timestamp_nanos`] expects.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the binary LLSD date is an f64; converting to integer nanoseconds is inherently lossy and bounded by the representable timestamp range"
)]
fn f64_seconds_to_nanos(seconds: f64) -> i128 {
    (seconds * 1_000_000_000.0) as i128
}

/// Parses a binary-LLSD byte stream into an [`Llsd`] value — the inverse of
/// [`Llsd::to_llsd_binary`].
///
/// Only the leading value is consumed; any trailing bytes (e.g. the newline
/// Firestorm's `saveToFile` appends after the cache map) are tolerated and
/// ignored, matching the reference loader.
///
/// # Errors
/// Returns [`LlsdError::TruncatedBinary`] on a short read,
/// [`LlsdError::UnknownBinaryMarker`] on an unrecognized type byte,
/// [`LlsdError::MissingBinaryTerminator`] on an unterminated array/map, or
/// [`LlsdError::InvalidBinaryDate`] on an out-of-range date.
pub fn parse_llsd_binary(bytes: &[u8]) -> Result<Llsd, LlsdError> {
    let mut cursor = Cursor::new(bytes);
    parse_value(&mut cursor)
}

/// A forward-only reader over a binary-LLSD byte slice that yields
/// [`LlsdError::TruncatedBinary`] instead of panicking on an out-of-bounds read.
struct Cursor<'a> {
    /// The full input being decoded.
    bytes: &'a [u8],
    /// The offset of the next unread byte.
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Wraps `bytes` at offset zero.
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Consumes and returns the next `n` bytes, or
    /// [`LlsdError::TruncatedBinary`] if fewer than `n` remain.
    fn take(&mut self, n: usize) -> Result<&'a [u8], LlsdError> {
        let end = self.pos.checked_add(n).ok_or(LlsdError::TruncatedBinary)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(LlsdError::TruncatedBinary)?;
        self.pos = end;
        Ok(slice)
    }

    /// Consumes and returns the next byte.
    fn take_u8(&mut self) -> Result<u8, LlsdError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(LlsdError::TruncatedBinary)
    }

    /// Consumes a 4-byte big-endian `u32` (a length or count prefix).
    fn take_u32(&mut self) -> Result<u32, LlsdError> {
        let array: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_ignored| LlsdError::TruncatedBinary)?;
        Ok(big_endian::u32_from(array))
    }

    /// Consumes a 4-byte big-endian `i32` integer value.
    fn take_i32(&mut self) -> Result<i32, LlsdError> {
        let array: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_ignored| LlsdError::TruncatedBinary)?;
        Ok(big_endian::i32_from(array))
    }

    /// Consumes an 8-byte big-endian `f64` real value.
    fn take_f64_be(&mut self) -> Result<f64, LlsdError> {
        let array: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_ignored| LlsdError::TruncatedBinary)?;
        Ok(big_endian::f64_from(array))
    }

    /// Consumes an 8-byte host-endian `f64` (the raw binary `Date` payload).
    fn take_f64_ne(&mut self) -> Result<f64, LlsdError> {
        let array: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_ignored| LlsdError::TruncatedBinary)?;
        Ok(f64::from_ne_bytes(array))
    }
}

/// Decodes the next binary-LLSD value at `cursor`.
fn parse_value(cursor: &mut Cursor<'_>) -> Result<Llsd, LlsdError> {
    let marker = cursor.take_u8()?;
    match marker {
        MARKER_UNDEF => Ok(Llsd::Undef),
        MARKER_TRUE => Ok(Llsd::Boolean(true)),
        MARKER_FALSE => Ok(Llsd::Boolean(false)),
        MARKER_INTEGER => Ok(Llsd::Integer(cursor.take_i32()?)),
        MARKER_REAL => Ok(Llsd::Real(cursor.take_f64_be()?)),
        MARKER_UUID => {
            let array: [u8; 16] = cursor
                .take(16)?
                .try_into()
                .map_err(|_ignored| LlsdError::TruncatedBinary)?;
            Ok(Llsd::Uuid(Uuid::from_bytes(array)))
        }
        MARKER_STRING => Ok(Llsd::String(parse_length_prefixed_string(cursor)?)),
        MARKER_URI => Ok(Llsd::Uri(parse_length_prefixed_string(cursor)?)),
        MARKER_DATE => Ok(Llsd::Date(epoch_to_date_string(cursor.take_f64_ne()?)?)),
        MARKER_BINARY => {
            let len = length_to_usize(cursor.take_u32()?)?;
            Ok(Llsd::Binary(cursor.take(len)?.to_vec()))
        }
        MARKER_ARRAY_BEGIN => parse_array(cursor),
        MARKER_MAP_BEGIN => parse_map(cursor),
        MARKER_NOTATION_SQUOTE | MARKER_NOTATION_DQUOTE => {
            Ok(Llsd::String(parse_delimited_string(cursor, marker)?))
        }
        other => Err(LlsdError::UnknownBinaryMarker { marker: other }),
    }
}

/// Decodes a binary-LLSD array body (the bytes after the `[` marker): a 4-byte
/// count, exactly that many values, then the mandatory `]` terminator.
fn parse_array(cursor: &mut Cursor<'_>) -> Result<Llsd, LlsdError> {
    let count = cursor.take_u32()?;
    let mut values = Vec::new();
    for _ in 0..count {
        values.push(parse_value(cursor)?);
    }
    expect_terminator(cursor, MARKER_ARRAY_END, ']')?;
    Ok(Llsd::Array(values))
}

/// Decodes a binary-LLSD map body (the bytes after the `{` marker): a 4-byte
/// count, exactly that many key/value pairs, then the mandatory `}` terminator.
fn parse_map(cursor: &mut Cursor<'_>) -> Result<Llsd, LlsdError> {
    let count = cursor.take_u32()?;
    let mut map = HashMap::new();
    for _ in 0..count {
        let key = parse_map_key(cursor)?;
        let value = parse_value(cursor)?;
        let _previous = map.insert(key, value);
    }
    expect_terminator(cursor, MARKER_MAP_END, '}')?;
    Ok(Llsd::Map(map))
}

/// Decodes a map key: the length-prefixed `k` form this codec emits, or a
/// tolerated notation-style `'` / `"` delimited key on read.
fn parse_map_key(cursor: &mut Cursor<'_>) -> Result<String, LlsdError> {
    let marker = cursor.take_u8()?;
    match marker {
        MARKER_MAP_KEY => parse_length_prefixed_string(cursor),
        MARKER_NOTATION_SQUOTE | MARKER_NOTATION_DQUOTE => parse_delimited_string(cursor, marker),
        other => Err(LlsdError::UnknownBinaryMarker { marker: other }),
    }
}

/// Reads the mandatory array/map terminator byte, erroring with
/// [`LlsdError::MissingBinaryTerminator`] if it is absent or the wrong byte.
fn expect_terminator(cursor: &mut Cursor<'_>, marker: u8, display: char) -> Result<(), LlsdError> {
    if cursor.take_u8()? == marker {
        Ok(())
    } else {
        Err(LlsdError::MissingBinaryTerminator { expected: display })
    }
}

/// Reads a 4-byte length prefix and the UTF-8 bytes that follow, decoding them
/// lossily (a malformed code unit becomes U+FFFD rather than an error, matching
/// the viewer's byte-for-byte string handling).
fn parse_length_prefixed_string(cursor: &mut Cursor<'_>) -> Result<String, LlsdError> {
    let len = length_to_usize(cursor.take_u32()?)?;
    let bytes = cursor.take(len)?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Reads a notation-style delimited string up to the matching closing `quote`,
/// honouring `\`-escapes — a read-only tolerance for Firestorm's notation form.
fn parse_delimited_string(cursor: &mut Cursor<'_>, quote: u8) -> Result<String, LlsdError> {
    let mut out = String::new();
    loop {
        let byte = cursor.take_u8()?;
        if byte == quote {
            return Ok(out);
        }
        if byte == b'\\' {
            out.push(decode_escape(cursor.take_u8()?));
        } else {
            out.push(char::from(byte));
        }
    }
}

/// Decodes a backslash escape inside a notation-style string. The common
/// whitespace escapes are recognised; any other escaped byte passes through
/// verbatim (so `\\` and `\"` yield the literal character).
fn decode_escape(byte: u8) -> char {
    match byte {
        b'n' => '\n',
        b't' => '\t',
        b'r' => '\r',
        other => char::from(other),
    }
}

/// Widens a wire `u32` length/count to `usize` (always lossless on the 32/64-bit
/// targets this runs on), mapping the impossible failure to
/// [`LlsdError::TruncatedBinary`] rather than panicking.
fn length_to_usize(len: u32) -> Result<usize, LlsdError> {
    usize::try_from(len).map_err(|_ignored| LlsdError::TruncatedBinary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Encodes then decodes `value`, asserting the decoded tree equals the
    /// original — the per-variant round-trip the cache relies on.
    ///
    /// # Errors
    /// Propagates a decode failure so callers can `?` it from a `Result`-typed
    /// test (the strict lints forbid `unwrap`/`expect`).
    fn round_trip(value: &Llsd) -> Result<(), LlsdError> {
        let encoded = value.to_llsd_binary();
        let decoded = parse_llsd_binary(&encoded)?;
        assert_eq!(&decoded, value);
        Ok(())
    }

    /// Every scalar [`Llsd`] variant round-trips through the binary codec
    /// individually (the date path is covered separately, since its string
    /// reformats on decode).
    #[test]
    fn scalar_variants_round_trip() -> Result<(), LlsdError> {
        round_trip(&Llsd::Undef)?;
        round_trip(&Llsd::Boolean(true))?;
        round_trip(&Llsd::Boolean(false))?;
        round_trip(&Llsd::Integer(0))?;
        round_trip(&Llsd::Integer(-1))?;
        round_trip(&Llsd::Integer(i32::MIN))?;
        round_trip(&Llsd::Integer(i32::MAX))?;
        round_trip(&Llsd::Real(0.0))?;
        round_trip(&Llsd::Real(-1.5))?;
        round_trip(&Llsd::Real(f64::MAX))?;
        round_trip(&Llsd::String(String::new()))?;
        round_trip(&Llsd::String("hello, region".to_owned()))?;
        round_trip(&Llsd::String("unicode ☺ é".to_owned()))?;
        round_trip(&Llsd::Uuid(Uuid::from_u128(0x0123_4567_89ab_cdef)))?;
        round_trip(&Llsd::Uuid(Uuid::nil()))?;
        round_trip(&Llsd::Uri("https://example.com/cap".to_owned()))?;
        round_trip(&Llsd::Binary(Vec::new()))?;
        round_trip(&Llsd::Binary(vec![0x00, 0x01, 0xfe, 0xff]))?;
        Ok(())
    }

    /// A nested array/map tree round-trips, exercising recursion, the 4-byte
    /// count prefixes and the mandatory terminators.
    #[test]
    fn nested_containers_round_trip() -> Result<(), LlsdError> {
        round_trip(&Llsd::Array(Vec::new()))?;
        round_trip(&Llsd::Map(HashMap::new()))?;
        let nested = Llsd::Map(HashMap::from([
            (
                "items".to_owned(),
                Llsd::Array(vec![
                    Llsd::Integer(1),
                    Llsd::String("a".to_owned()),
                    Llsd::Array(vec![Llsd::Boolean(true), Llsd::Undef]),
                ]),
            ),
            (
                "meta".to_owned(),
                Llsd::Map(HashMap::from([
                    ("count".to_owned(), Llsd::Integer(2)),
                    ("id".to_owned(), Llsd::Uuid(Uuid::from_u128(7))),
                ])),
            ),
        ]));
        round_trip(&nested)?;
        Ok(())
    }

    /// The inventory-cache map shape (`{ categories: [...], items: [...] }`)
    /// round-trips. Item creation dates serialise as LLSD `Integer`, not `Date`,
    /// so the cache map never exercises the date path.
    #[test]
    fn cache_map_shape_round_trips() -> Result<(), LlsdError> {
        let category = Llsd::Map(HashMap::from([
            ("cat_id".to_owned(), Llsd::Uuid(Uuid::from_u128(1))),
            ("parent_id".to_owned(), Llsd::Uuid(Uuid::nil())),
            ("name".to_owned(), Llsd::String("Objects".to_owned())),
            ("version".to_owned(), Llsd::Integer(42)),
            ("type_default".to_owned(), Llsd::Integer(6)),
        ]));
        let item = Llsd::Map(HashMap::from([
            ("item_id".to_owned(), Llsd::Uuid(Uuid::from_u128(2))),
            ("parent_id".to_owned(), Llsd::Uuid(Uuid::from_u128(1))),
            ("name".to_owned(), Llsd::String("a cube".to_owned())),
            ("created_at".to_owned(), Llsd::Integer(1_700_000_000)),
            ("asset_id".to_owned(), Llsd::Uuid(Uuid::from_u128(3))),
        ]));
        let cache = Llsd::Map(HashMap::from([
            ("categories".to_owned(), Llsd::Array(vec![category])),
            ("items".to_owned(), Llsd::Array(vec![item])),
        ]));
        round_trip(&cache)?;
        Ok(())
    }

    /// Decoding the binary encoding of a shared fixture yields the same tree as
    /// parsing its XML encoding — a cross-check that the two codecs agree.
    #[test]
    fn binary_matches_xml_for_shared_fixture() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = Llsd::Map(HashMap::from([
            ("flag".to_owned(), Llsd::Boolean(true)),
            ("count".to_owned(), Llsd::Integer(-7)),
            ("ratio".to_owned(), Llsd::Real(0.25)),
            ("name".to_owned(), Llsd::String("parcel".to_owned())),
            ("id".to_owned(), Llsd::Uuid(Uuid::from_u128(0xfeed))),
            ("blob".to_owned(), Llsd::Binary(vec![1, 2, 3])),
            (
                "list".to_owned(),
                Llsd::Array(vec![Llsd::Undef, Llsd::Integer(9)]),
            ),
        ]));

        let from_binary = parse_llsd_binary(&fixture.to_llsd_binary())?;
        let from_xml = crate::parse_llsd_xml(&fixture.to_llsd_xml())?;
        assert_eq!(from_binary, from_xml);
        Ok(())
    }

    /// A `Date` round-trips through epoch-seconds: re-encoding the decoded value
    /// reproduces the original bytes (the ISO string itself reformats on the
    /// way back, so byte-idempotency is the stable invariant).
    #[test]
    fn date_round_trips_through_epoch() -> Result<(), LlsdError> {
        let original = Llsd::Date("2026-06-28T12:34:56Z".to_owned());
        let encoded = original.to_llsd_binary();
        let decoded = parse_llsd_binary(&encoded)?;
        assert!(matches!(decoded, Llsd::Date(_)));
        assert_eq!(decoded.to_llsd_binary(), encoded);
        Ok(())
    }

    /// Truncated, unknown-tag, missing-terminator and count-mismatch inputs all
    /// return `Err` without panicking (no indexing/slicing panic).
    #[test]
    fn malformed_inputs_error_without_panicking() {
        // Empty input — nothing to read.
        assert_eq!(parse_llsd_binary(&[]), Err(LlsdError::TruncatedBinary));
        // Integer marker with a short payload.
        assert_eq!(
            parse_llsd_binary(&[MARKER_INTEGER, 0x00, 0x01]),
            Err(LlsdError::TruncatedBinary)
        );
        // Unrecognized marker byte.
        assert_eq!(
            parse_llsd_binary(b"Z"),
            Err(LlsdError::UnknownBinaryMarker { marker: b'Z' })
        );
        // Array declaring one element followed by a present-but-wrong byte
        // where the `]` terminator should be.
        let mut unterminated = vec![MARKER_ARRAY_BEGIN];
        unterminated.extend_from_slice(&big_endian::u32_to(1));
        unterminated.push(MARKER_UNDEF);
        unterminated.push(MARKER_UNDEF);
        assert_eq!(
            parse_llsd_binary(&unterminated),
            Err(LlsdError::MissingBinaryTerminator { expected: ']' })
        );
        // Map declaring two entries but only carrying one (count mismatch ⇒ the
        // second key read lands on the `}` terminator byte, an invalid marker).
        let mut short_map = vec![MARKER_MAP_BEGIN];
        short_map.extend_from_slice(&big_endian::u32_to(2));
        push_binary_string(&mut short_map, MARKER_MAP_KEY, "only");
        short_map.push(MARKER_UNDEF);
        short_map.push(MARKER_MAP_END);
        assert_eq!(
            parse_llsd_binary(&short_map),
            Err(LlsdError::UnknownBinaryMarker {
                marker: MARKER_MAP_END
            })
        );
        // String declaring more bytes than remain.
        let mut short_string = vec![MARKER_STRING];
        short_string.extend_from_slice(&big_endian::u32_to(64));
        short_string.extend_from_slice(b"short");
        assert_eq!(
            parse_llsd_binary(&short_string),
            Err(LlsdError::TruncatedBinary)
        );
    }

    /// The decoder tolerates Firestorm's notation-style quoted strings and map
    /// keys on read, even though this codec only ever emits the length-prefixed
    /// `s` / `k` forms.
    #[test]
    fn tolerates_notation_strings_on_read() -> Result<(), LlsdError> {
        // A single-quoted notation string value.
        let mut notation = vec![MARKER_NOTATION_SQUOTE];
        notation.extend_from_slice(b"hi");
        notation.push(MARKER_NOTATION_SQUOTE);
        assert_eq!(parse_llsd_binary(&notation)?, Llsd::String("hi".to_owned()));

        // A map with a double-quoted key.
        let mut map = vec![MARKER_MAP_BEGIN];
        map.extend_from_slice(&big_endian::u32_to(1));
        map.push(MARKER_NOTATION_DQUOTE);
        map.extend_from_slice(b"key");
        map.push(MARKER_NOTATION_DQUOTE);
        map.push(MARKER_INTEGER);
        map.extend_from_slice(&big_endian::i32_to(5));
        map.push(MARKER_MAP_END);
        assert_eq!(
            parse_llsd_binary(&map)?,
            Llsd::Map(HashMap::from([("key".to_owned(), Llsd::Integer(5))]))
        );
        Ok(())
    }

    /// Trailing bytes after the top-level value (e.g. the newline Firestorm
    /// appends to the cache file) are tolerated and ignored.
    #[test]
    fn tolerates_trailing_bytes() -> Result<(), LlsdError> {
        let mut bytes = Llsd::Integer(3).to_llsd_binary();
        bytes.push(b'\n');
        assert_eq!(parse_llsd_binary(&bytes)?, Llsd::Integer(3));
        Ok(())
    }
}
