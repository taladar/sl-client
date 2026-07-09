//! Notation-LLSD reading — a minimal cursor ([`Scan`]) plus a full parser
//! ([`parse_llsd_notation`]).
//!
//! Notation LLSD is the textual serialization Second Life / OpenSim use for some
//! payloads (e.g. the GLTF material-override `GenericStreamingMessage`). [`Scan`]
//! is sufficient to walk such a stream and slice out (without interpreting)
//! nested values; [`parse_llsd_notation`] interprets the whole stream into an
//! [`Llsd`] tree, the textual counterpart of
//! [`parse_llsd_binary`](crate::parse_llsd_binary).

use std::collections::HashMap;

use base64::Engine as _;
use uuid::Uuid;

use crate::error::LlsdError;
use crate::value::Llsd;

/// A minimal cursor over a notation-LLSD byte slice, sufficient to walk a value
/// and slice out (without interpreting) nested values.
#[derive(Debug)]
pub struct Scan<'a> {
    /// The backing buffer.
    buf: &'a [u8],
    /// The current offset into `buf`.
    pos: usize,
}

impl<'a> Scan<'a> {
    /// Creates a scanner over `buf`, positioned at its start.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Returns the byte at the cursor without advancing.
    #[must_use]
    pub fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    /// Advances the cursor by one byte (saturating at the buffer end).
    pub const fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    /// Skips ASCII whitespace and element separators (commas).
    pub fn skip_ws_sep(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n' | b',')) {
            self.bump();
        }
    }

    /// Skips whitespace, then consumes `byte` if present, returning `None`
    /// otherwise.
    pub fn expect(&mut self, byte: u8) -> Option<()> {
        self.skip_ws_sep();
        if self.peek()? == byte {
            self.bump();
            Some(())
        } else {
            None
        }
    }

    /// Reads a notation string token (`'…'` or `"…"`), honouring `\` escapes.
    pub fn read_quoted_string(&mut self) -> Option<String> {
        self.skip_ws_sep();
        let quote = self.peek()?;
        if quote != b'\'' && quote != b'"' {
            return None;
        }
        self.bump();
        let mut out = Vec::new();
        loop {
            let byte = self.peek()?;
            self.bump();
            match byte {
                b'\\' => {
                    let escaped = self.peek()?;
                    self.bump();
                    out.push(escaped);
                }
                b if b == quote => break,
                b => out.push(b),
            }
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }

    /// Reads a notation integer token (`i<digits>`, optionally signed).
    pub fn read_integer(&mut self) -> Option<i64> {
        self.expect(b'i')?;
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.bump();
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        let digits = self.buf.get(start..self.pos)?;
        std::str::from_utf8(digits).ok()?.parse().ok()
    }

    /// Reads a notation array of integers (`[ i1, i2, … ]`).
    pub fn read_integer_array(&mut self) -> Option<Vec<i64>> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        loop {
            self.skip_ws_sep();
            if self.peek()? == b']' {
                self.bump();
                break;
            }
            out.push(self.read_integer()?);
        }
        Some(out)
    }

    /// Reads a notation array, returning each element's raw bytes verbatim (used
    /// for values that are left uninterpreted by this layer).
    pub fn read_raw_array(&mut self) -> Option<Vec<Vec<u8>>> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        loop {
            self.skip_ws_sep();
            if self.peek()? == b']' {
                self.bump();
                break;
            }
            let (start, end) = self.skip_value()?;
            out.push(self.buf.get(start..end)?.to_vec());
        }
        Some(out)
    }

    /// Advances past one complete notation value, returning its `(start, end)`
    /// byte range. Nested maps/arrays and quoted strings are balanced so that
    /// delimiters inside strings are not mistaken for structure.
    pub fn skip_value(&mut self) -> Option<(usize, usize)> {
        self.skip_ws_sep();
        let start = self.pos;
        match self.peek()? {
            b'!' => self.bump(),
            b'0' | b'1' | b't' | b'f' | b'T' | b'F' => self.skip_token(),
            b'i' | b'r' => {
                self.bump();
                self.skip_number();
            }
            b'u' => {
                self.bump();
                self.skip_uuid();
            }
            b'\'' | b'"' => {
                self.read_quoted_string()?;
            }
            b'l' | b'd' => {
                self.bump();
                self.read_quoted_string()?;
            }
            b's' | b'b' => self.skip_sized(),
            b'[' => {
                self.bump();
                loop {
                    self.skip_ws_sep();
                    if self.peek()? == b']' {
                        self.bump();
                        break;
                    }
                    self.skip_value()?;
                }
            }
            b'{' => {
                self.bump();
                loop {
                    self.skip_ws_sep();
                    if self.peek()? == b'}' {
                        self.bump();
                        break;
                    }
                    self.read_quoted_string()?;
                    self.expect(b':')?;
                    self.skip_value()?;
                }
            }
            _ => return None,
        }
        Some((start, self.pos))
    }

    /// Consumes a run of ASCII letters/digits (a bare boolean keyword).
    fn skip_token(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z')) {
            self.bump();
        }
    }

    /// Consumes a numeric run (sign, digits, decimal point and exponent).
    fn skip_number(&mut self) {
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.bump();
        }
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.bump();
        }
    }

    /// Consumes a UUID run (hexadecimal digits and dashes).
    fn skip_uuid(&mut self) {
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b'-')
        ) {
            self.bump();
        }
    }

    /// Consumes a size-prefixed string or binary token (`s(len)"…"`,
    /// `b(len)"…"`, `b16"…"` or `b64"…"`).
    fn skip_sized(&mut self) {
        self.bump();
        // Optional size or radix marker before the quoted body.
        while matches!(self.peek(), Some(b'0'..=b'9' | b'(' | b')')) {
            self.bump();
        }
        self.read_quoted_string();
    }
}

/// Parses a notation-LLSD byte stream into an [`Llsd`] value — the textual
/// counterpart of [`parse_llsd_binary`](crate::parse_llsd_binary), mirroring
/// Firestorm's `LLSDNotationParser` (`indra/llcommon/llsdserialize.cpp`).
///
/// Notation is the format the simulator uses for the GLTF material-override
/// `GenericStreamingMessage` (each per-face override document is a notation map),
/// so this is what a material-override decoder reads. Every LLSD kind is
/// supported: `!` undef, `0` / `1` / `true` / `false` booleans, `i####`
/// integers, `r####` reals, `u####` uuids, `'…'` / `"…"` / `s(len)"…"` strings,
/// `l"…"` uris, `d"…"` dates, `b(len)"…"` / `b16"…"` / `b64"…"` binaries, `[ … ]`
/// arrays and `{ 'k':v, … }` maps.
///
/// # Errors
///
/// Returns [`LlsdError::MalformedNotation`] if the stream ends mid-value or a
/// byte does not begin a valid notation value.
pub fn parse_llsd_notation(bytes: &[u8]) -> Result<Llsd, LlsdError> {
    let mut parser = NotationParser { buf: bytes, pos: 0 };
    parser.parse_value()
}

/// A recursive-descent cursor over a notation-LLSD byte slice, producing an
/// owned [`Llsd`] tree.
struct NotationParser<'a> {
    /// The backing buffer.
    buf: &'a [u8],
    /// The current offset into `buf`.
    pos: usize,
}

impl NotationParser<'_> {
    /// The byte at the cursor, or `None` at end of input.
    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    /// Advances the cursor by one byte (saturating at the buffer end).
    const fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    /// Skips ASCII whitespace and element separators (commas).
    fn skip_ws_sep(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n' | b',')) {
            self.bump();
        }
    }

    /// Consumes `byte` after leading whitespace, erroring if it is not next.
    fn expect(&mut self, byte: u8) -> Result<(), LlsdError> {
        self.skip_ws_sep();
        if self.peek() == Some(byte) {
            self.bump();
            Ok(())
        } else {
            Err(LlsdError::MalformedNotation)
        }
    }

    /// Parses one complete notation value at the cursor.
    fn parse_value(&mut self) -> Result<Llsd, LlsdError> {
        self.skip_ws_sep();
        match self.peek().ok_or(LlsdError::MalformedNotation)? {
            b'{' => self.parse_map(),
            b'[' => self.parse_array(),
            b'!' => {
                self.bump();
                Ok(Llsd::Undef)
            }
            b'0' => {
                self.bump();
                Ok(Llsd::Boolean(false))
            }
            b'1' => {
                self.bump();
                Ok(Llsd::Boolean(true))
            }
            b't' | b'T' => {
                self.bump();
                self.skip_alpha();
                Ok(Llsd::Boolean(true))
            }
            b'f' | b'F' => {
                self.bump();
                self.skip_alpha();
                Ok(Llsd::Boolean(false))
            }
            b'i' => self.parse_integer(),
            b'r' => self.parse_real(),
            b'u' => self.parse_uuid(),
            b'\'' | b'"' => Ok(Llsd::String(self.parse_quoted()?)),
            b's' => Ok(Llsd::String(self.parse_sized_string()?)),
            b'l' => Ok(Llsd::Uri(self.parse_delimited_after_marker()?)),
            b'd' => Ok(Llsd::Date(self.parse_delimited_after_marker()?)),
            b'b' => self.parse_binary(),
            _ => Err(LlsdError::MalformedNotation),
        }
    }

    /// Parses a `{ 'key':value, … }` map.
    fn parse_map(&mut self) -> Result<Llsd, LlsdError> {
        self.expect(b'{')?;
        let mut map = HashMap::new();
        loop {
            self.skip_ws_sep();
            match self.peek().ok_or(LlsdError::MalformedNotation)? {
                b'}' => {
                    self.bump();
                    break;
                }
                b'\'' | b'"' | b's' => {}
                _ => return Err(LlsdError::MalformedNotation),
            }
            let key = match self.peek() {
                Some(b's') => self.parse_sized_string()?,
                _ => self.parse_quoted()?,
            };
            self.expect(b':')?;
            let value = self.parse_value()?;
            let _prev = map.insert(key, value);
        }
        Ok(Llsd::Map(map))
    }

    /// Parses a `[ value, … ]` array.
    fn parse_array(&mut self) -> Result<Llsd, LlsdError> {
        self.expect(b'[')?;
        let mut array = Vec::new();
        loop {
            self.skip_ws_sep();
            if self.peek().ok_or(LlsdError::MalformedNotation)? == b']' {
                self.bump();
                break;
            }
            array.push(self.parse_value()?);
        }
        Ok(Llsd::Array(array))
    }

    /// Parses an `i####` integer (leniently narrowed to `i32`).
    fn parse_integer(&mut self) -> Result<Llsd, LlsdError> {
        self.expect(b'i')?;
        let token = self.take_number_token();
        let value: i64 = token
            .parse()
            .map_err(|_parse| LlsdError::MalformedNotation)?;
        Ok(Llsd::Integer(narrow_to_i32(value)))
    }

    /// Parses an `r####` real.
    fn parse_real(&mut self) -> Result<Llsd, LlsdError> {
        self.expect(b'r')?;
        let token = self.take_number_token();
        let value: f64 = token
            .parse()
            .map_err(|_parse| LlsdError::MalformedNotation)?;
        Ok(Llsd::Real(value))
    }

    /// Parses a `u####` uuid (36-char hyphenated form).
    fn parse_uuid(&mut self) -> Result<Llsd, LlsdError> {
        self.expect(b'u')?;
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b'-')
        ) {
            self.bump();
        }
        let token = self
            .buf
            .get(start..self.pos)
            .ok_or(LlsdError::MalformedNotation)?;
        let text = str::from_utf8(token).map_err(|_utf8| LlsdError::MalformedNotation)?;
        let uuid = Uuid::parse_str(text).map_err(|_uuid| LlsdError::MalformedNotation)?;
        Ok(Llsd::Uuid(uuid))
    }

    /// Reads a signed numeric run (sign, digits, decimal point, exponent) as a
    /// UTF-8 string for the caller to parse.
    fn take_number_token(&mut self) -> String {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.bump();
        }
        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.bump();
        }
        String::from_utf8_lossy(self.buf.get(start..self.pos).unwrap_or(&[])).into_owned()
    }

    /// Consumes a run of ASCII letters (the tail of a `true` / `false` keyword).
    fn skip_alpha(&mut self) {
        while matches!(self.peek(), Some(b'a'..=b'z' | b'A'..=b'Z')) {
            self.bump();
        }
    }

    /// Parses a `'…'` / `"…"` delimited string at the cursor, honouring escapes.
    fn parse_quoted(&mut self) -> Result<String, LlsdError> {
        self.skip_ws_sep();
        let quote = self.peek().ok_or(LlsdError::MalformedNotation)?;
        if quote != b'\'' && quote != b'"' {
            return Err(LlsdError::MalformedNotation);
        }
        self.bump();
        self.read_until_delim(quote)
    }

    /// Parses a `l"…"` / `d"…"` value: pops the marker, then the delimiter, then
    /// the delimited body.
    fn parse_delimited_after_marker(&mut self) -> Result<String, LlsdError> {
        self.bump(); // the `l` / `d` marker
        let delim = self.peek().ok_or(LlsdError::MalformedNotation)?;
        if delim != b'\'' && delim != b'"' {
            return Err(LlsdError::MalformedNotation);
        }
        self.bump();
        self.read_until_delim(delim)
    }

    /// Reads bytes up to (and consuming) the closing `delim`, decoding the
    /// notation escape sequences (`\xHH` hex, `\a\b\f\n\r\t\v`, and `\<char>`).
    fn read_until_delim(&mut self, delim: u8) -> Result<String, LlsdError> {
        let mut out: Vec<u8> = Vec::new();
        loop {
            let byte = self.peek().ok_or(LlsdError::MalformedNotation)?;
            self.bump();
            match byte {
                b'\\' => {
                    let escaped = self.peek().ok_or(LlsdError::MalformedNotation)?;
                    self.bump();
                    match escaped {
                        b'x' => {
                            let high = self.hex_nibble()?;
                            let low = self.hex_nibble()?;
                            out.push(high.wrapping_shl(4) | low);
                        }
                        b'a' => out.push(0x07),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'v' => out.push(0x0b),
                        other => out.push(other),
                    }
                }
                b if b == delim => break,
                b => out.push(b),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Reads one hexadecimal nibble (the digit of a `\xHH` escape).
    fn hex_nibble(&mut self) -> Result<u8, LlsdError> {
        let byte = self.peek().ok_or(LlsdError::MalformedNotation)?;
        let nibble = hex_value(byte).ok_or(LlsdError::MalformedNotation)?;
        self.bump();
        Ok(nibble)
    }

    /// Parses a `s(len)"raw"` sized string (the length is advisory — the body
    /// runs to its closing delimiter, matching the escape-aware reader).
    fn parse_sized_string(&mut self) -> Result<String, LlsdError> {
        self.expect(b's')?;
        // A `(len)` prefix (the reference emits it); tolerate its absence.
        if self.peek() == Some(b'(') {
            while !matches!(self.peek(), Some(b')') | None) {
                self.bump();
            }
            self.expect(b')')?;
        }
        let delim = self.peek().ok_or(LlsdError::MalformedNotation)?;
        if delim != b'\'' && delim != b'"' {
            return Err(LlsdError::MalformedNotation);
        }
        self.bump();
        self.read_until_delim(delim)
    }

    /// Parses a `b(len)"raw"`, `b16"…"`, or `b64"…"` binary value.
    fn parse_binary(&mut self) -> Result<Llsd, LlsdError> {
        self.expect(b'b')?;
        match self.peek().ok_or(LlsdError::MalformedNotation)? {
            b'(' => {
                // Raw byte count in parentheses, then a quoted (unescaped) body.
                self.bump();
                let start = self.pos;
                while !matches!(self.peek(), Some(b')') | None) {
                    self.bump();
                }
                let len_text =
                    String::from_utf8_lossy(self.buf.get(start..self.pos).unwrap_or(&[]))
                        .into_owned();
                self.expect(b')')?;
                let len: usize = len_text
                    .parse()
                    .map_err(|_parse| LlsdError::MalformedNotation)?;
                let delim = self.peek().ok_or(LlsdError::MalformedNotation)?;
                if delim != b'\'' && delim != b'"' {
                    return Err(LlsdError::MalformedNotation);
                }
                self.bump();
                let body_start = self.pos;
                let body_end = body_start.saturating_add(len);
                let bytes = self
                    .buf
                    .get(body_start..body_end)
                    .ok_or(LlsdError::MalformedNotation)?
                    .to_vec();
                self.pos = body_end;
                self.expect(delim)?;
                Ok(Llsd::Binary(bytes))
            }
            b'1' | b'6' => {
                // `b16"…"` — hex-encoded body up to the closing delimiter.
                self.take_radix_marker();
                let text = self.parse_quoted()?;
                Ok(Llsd::Binary(decode_hex(&text)))
            }
            _ => {
                // `b64"…"` — standard base64 body up to the closing delimiter.
                self.take_radix_marker();
                let text = self.parse_quoted()?;
                Ok(Llsd::Binary(
                    base64::engine::general_purpose::STANDARD
                        .decode(text.trim())
                        .unwrap_or_default(),
                ))
            }
        }
    }

    /// Consumes the digits of a `b16` / `b64` radix marker (leaving the cursor at
    /// the opening quote).
    fn take_radix_marker(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
    }
}

/// Decodes a run of hexadecimal digit pairs into bytes, ignoring a trailing odd
/// nibble (mirroring the reference's tolerant `b16` reader).
fn decode_hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter_map(hex_value).collect();
    digits
        .chunks_exact(2)
        .filter_map(|pair| match pair {
            [high, low] => Some(high.wrapping_shl(4) | *low),
            _ => None,
        })
        .collect()
}

/// The numeric value `0..=15` of one ASCII hexadecimal digit, or `None`.
const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(byte.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(byte.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

/// Narrows a parsed notation integer (`i64`) to the `i32` LLSD integers carry,
/// saturating out-of-range values.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "LLSD integers are i32; the wide parse clamps rather than wraps"
)]
const fn narrow_to_i32(value: i64) -> i32 {
    if value > i32::MAX as i64 {
        i32::MAX
    } else if value < i32::MIN as i64 {
        i32::MIN
    } else {
        value as i32
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::parse_llsd_notation;
    use crate::error::LlsdError;
    use crate::value::Llsd;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Each scalar kind round-trips through the notation reader with its value
    /// intact (booleans in both single-char and keyword forms).
    #[test]
    fn parses_scalars() -> Result<(), TestError> {
        assert_eq!(parse_llsd_notation(b"!")?, Llsd::Undef);
        assert_eq!(parse_llsd_notation(b"1")?, Llsd::Boolean(true));
        assert_eq!(parse_llsd_notation(b"0")?, Llsd::Boolean(false));
        assert_eq!(parse_llsd_notation(b"true")?, Llsd::Boolean(true));
        assert_eq!(parse_llsd_notation(b"false")?, Llsd::Boolean(false));
        assert_eq!(parse_llsd_notation(b"i-42")?, Llsd::Integer(-42));
        assert_eq!(parse_llsd_notation(b"r0.25")?, Llsd::Real(0.25));
        assert_eq!(
            parse_llsd_notation(b"'hi there'")?,
            Llsd::String("hi there".to_owned())
        );
        let uuid = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(
            parse_llsd_notation(format!("u{uuid}").as_bytes())?,
            Llsd::Uuid(Uuid::parse_str(uuid)?)
        );
        Ok(())
    }

    /// A nested map/array with mixed value kinds parses into the matching
    /// [`Llsd`] tree — the shape a GLTF material-override document takes.
    #[test]
    fn parses_nested_map() -> Result<(), TestError> {
        let notation = b"{'mf':r0.5,'am':i1,'ds':1,'ti':[{'o':[r0.1,r0.2]}]}";
        let value = parse_llsd_notation(notation)?;
        assert_eq!(value.field_f32("mf", "mf")?, Some(0.5));
        assert_eq!(value.field_i32("am", "am")?, Some(1));
        assert_eq!(value.field_bool("ds", "ds")?, Some(true));
        let ti = value.get("ti").and_then(Llsd::as_array).ok_or("no ti")?;
        let offset = ti
            .first()
            .and_then(|entry| entry.get("o"))
            .and_then(Llsd::as_array)
            .ok_or("no offset")?;
        assert_eq!(offset.len(), 2);
        assert_eq!(offset.first().and_then(Llsd::as_f32), Some(0.1));
        Ok(())
    }

    /// Double-quoted strings and `\`-escapes (both the named `\n` and a literal
    /// `\'`) decode correctly.
    #[test]
    fn parses_escaped_strings() -> Result<(), TestError> {
        assert_eq!(
            parse_llsd_notation(b"\"a\\nb\"")?,
            Llsd::String("a\nb".to_owned())
        );
        assert_eq!(
            parse_llsd_notation(b"'it\\'s'")?,
            Llsd::String("it's".to_owned())
        );
        Ok(())
    }

    /// A binary `b(len)"raw"` value reads exactly `len` bytes verbatim.
    #[test]
    fn parses_sized_binary() -> Result<(), TestError> {
        assert_eq!(
            parse_llsd_notation(b"b(3)\"abc\"")?,
            Llsd::Binary(b"abc".to_vec())
        );
        Ok(())
    }

    /// A truncated stream and an unrecognised leading byte are both hard errors,
    /// not a silently defaulted value.
    #[test]
    fn rejects_malformed() {
        assert_eq!(
            parse_llsd_notation(b"{'k':"),
            Err(LlsdError::MalformedNotation)
        );
        assert_eq!(parse_llsd_notation(b"@"), Err(LlsdError::MalformedNotation));
    }
}
