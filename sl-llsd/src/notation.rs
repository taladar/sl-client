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
    /// How many arrays / maps are currently open above this point.
    depth: usize,
}

impl<'a> Scan<'a> {
    /// Creates a scanner over `buf`, positioned at its start.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            depth: 0,
        }
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
        // `[` and `{` below recurse into `skip_value`, so this scanner's depth
        // is the thread's stack depth. `None` is this scanner's error channel.
        if self.depth >= crate::MAX_NESTING_DEPTH {
            return None;
        }
        self.depth = self.depth.saturating_add(1);
        let range = self.skip_value_inner();
        self.depth = self.depth.saturating_sub(1);
        range
    }

    /// The body of [`skip_value`](Self::skip_value), with the depth guard
    /// already applied.
    fn skip_value_inner(&mut self) -> Option<(usize, usize)> {
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
    let mut parser = NotationParser {
        buf: bytes,
        pos: 0,
        depth: 0,
    };
    parser.parse_value()
}

/// Serializes `value` as notation LLSD — the inverse of
/// [`parse_llsd_notation`], mirroring Firestorm's `LLSDNotationFormatter`
/// (`indra/llcommon/llsdserialize.cpp`) at its default options.
///
/// Bytes rather than a `String`: the reference escapes every byte outside
/// printable ASCII, so the output is ASCII, but the *binary* kind is written
/// as `b16"…"` uppercase hex (the reference's `OPTIONS_PRETTY_BINARY`, which
/// is the default `LLSDSerialize::serialize` passes) and the caller usually
/// wants bytes anyway.
///
/// Map keys are emitted in sorted order, matching
/// [`to_llsd_xml`](Llsd::to_llsd_xml) and
/// [`to_llsd_binary`](Llsd::to_llsd_binary), so two equal trees serialize
/// identically.
pub(crate) fn to_notation(value: &Llsd) -> Vec<u8> {
    let mut out = Vec::new();
    push_notation(value, &mut out);
    out
}

/// Appends `value`'s notation encoding to `out`, recursing into arrays and
/// maps. The value-by-value inverse of
/// [`parse_value`](NotationParser::parse_value).
fn push_notation(value: &Llsd, out: &mut Vec<u8>) {
    match value {
        Llsd::Undef => out.push(b'!'),
        // The reference's default formatter has `boolalpha` off, so a boolean
        // is the bare digit — `i1` would be an integer, `1` is `true`.
        Llsd::Boolean(flag) => out.push(if *flag { b'1' } else { b'0' }),
        Llsd::Integer(integer) => {
            out.push(b'i');
            out.extend_from_slice(integer.to_string().as_bytes());
        }
        Llsd::Real(real) => {
            out.push(b'r');
            // The reference streams a real at the ostream default of six
            // significant digits, which loses precision; Rust's shortest
            // round-tripping form is a deliberate improvement on it (the
            // parser reads either).
            out.extend_from_slice(real.to_string().as_bytes());
        }
        Llsd::Uuid(uuid) => {
            out.push(b'u');
            out.extend_from_slice(uuid.to_string().as_bytes());
        }
        Llsd::String(string) => push_notation_string(out, b'\'', string),
        Llsd::Uri(uri) => {
            out.push(b'l');
            push_notation_string(out, b'"', uri);
        }
        // A date is kept verbatim (it is already the ISO-8601 text the
        // reference streams out of `LLDate`).
        Llsd::Date(date) => {
            out.push(b'd');
            push_notation_string(out, b'"', date);
        }
        Llsd::Binary(blob) => {
            out.extend_from_slice(b"b16\"");
            for byte in blob {
                // Uppercase: the reference notes Python's `llbase.llsd`
                // rejects lowercase `b16` digits, so it emits them uppercase.
                out.push(hex_digit(byte.wrapping_shr(4), b'A'));
                out.push(hex_digit(byte & 0x0f, b'A'));
            }
            out.push(b'"');
        }
        Llsd::Array(values) => {
            out.push(b'[');
            for (index, element) in values.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                push_notation(element, out);
            }
            out.push(b']');
        }
        Llsd::Map(map) => {
            out.push(b'{');
            let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            for (index, (key, member)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                push_notation_string(out, b'\'', key);
                out.push(b':');
                push_notation(member, out);
            }
            out.push(b'}');
        }
    }
}

/// Appends `value` delimited by `quote`, escaping it the way the reference's
/// `NOTATION_STRING_CHARACTERS` table does: the named escapes for `\a\b\t\n\v\f\r`,
/// `\xHH` for every other byte below `0x20` and every byte from `0x7f` up (so a
/// multi-byte UTF-8 sequence leaves as escaped bytes and returns as itself), and
/// a backslash before a literal backslash or the delimiter.
fn push_notation_string(out: &mut Vec<u8>, quote: u8, value: &str) {
    out.push(quote);
    for byte in value.bytes() {
        match byte {
            0x07 => out.extend_from_slice(b"\\a"),
            0x08 => out.extend_from_slice(b"\\b"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            0x0b => out.extend_from_slice(b"\\v"),
            0x0c => out.extend_from_slice(b"\\f"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            other if other == quote => {
                out.push(b'\\');
                out.push(other);
            }
            other if !(0x20..0x7f).contains(&other) => {
                out.extend_from_slice(b"\\x");
                out.push(hex_digit(other.wrapping_shr(4), b'a'));
                out.push(hex_digit(other & 0x0f, b'a'));
            }
            other => out.push(other),
        }
    }
    out.push(quote);
}

/// The ASCII hexadecimal digit for a nibble `0..=15`, with `ten` as the letter
/// the digits above nine start from (`b'a'` or `b'A'`).
const fn hex_digit(nibble: u8, ten: u8) -> u8 {
    match nibble {
        0..=9 => b'0'.wrapping_add(nibble),
        _ => ten.wrapping_add(nibble.wrapping_sub(10)),
    }
}

/// A recursive-descent cursor over a notation-LLSD byte slice, producing an
/// owned [`Llsd`] tree.
struct NotationParser<'a> {
    /// The backing buffer.
    buf: &'a [u8],
    /// The current offset into `buf`.
    pos: usize,
    /// How many arrays / maps are currently open above this point.
    depth: usize,
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

    /// Opens a container, rejecting input that nests past
    /// [`MAX_NESTING_DEPTH`](crate::MAX_NESTING_DEPTH).
    ///
    /// Checked on the way *in*: this recursion's depth is the thread's stack
    /// depth, and notation costs a single byte per level, so an unbounded one
    /// is a cheap remote crash rather than a catchable error.
    const fn enter(&mut self) -> Result<(), LlsdError> {
        if self.depth >= crate::MAX_NESTING_DEPTH {
            return Err(LlsdError::NestingTooDeep {
                limit: crate::MAX_NESTING_DEPTH,
            });
        }
        self.depth = self.depth.saturating_add(1);
        Ok(())
    }

    /// Closes a container opened by [`enter`](Self::enter). Only the success
    /// path unwinds the counter; an error abandons the whole parse.
    const fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
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
        self.enter()?;
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
        self.leave();
        Ok(Llsd::Map(map))
    }

    /// Parses a `[ value, … ]` array.
    fn parse_array(&mut self) -> Result<Llsd, LlsdError> {
        self.enter()?;
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
        self.leave();
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
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[high, low]| high.wrapping_shl(4) | low)
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

    /// Notation costs a **single byte** per nesting level, so an unbounded
    /// parser is the cheapest remote crash in the crate. 100_000 levels is
    /// 100 kB of input; without the guard it is a SIGSEGV.
    #[test]
    fn nesting_past_the_limit_is_rejected_not_a_stack_overflow() {
        let bytes = "[".repeat(100_000);
        assert_eq!(
            parse_llsd_notation(bytes.as_bytes()),
            Err(LlsdError::NestingTooDeep {
                limit: crate::MAX_NESTING_DEPTH,
            })
        );
    }

    /// The scanner recurses through `skip_value` for the same reason, and
    /// reports the refusal through its own `None` channel.
    #[test]
    fn the_scanner_refuses_to_skip_past_the_limit() {
        let bytes = "[".repeat(100_000);
        assert_eq!(super::Scan::new(bytes.as_bytes()).skip_value(), None);
    }

    /// Nesting the protocol actually produces still parses.
    #[test]
    fn ordinary_nesting_is_untouched_by_the_limit() {
        let bytes = format!("{}i7{}", "[".repeat(16), "]".repeat(16));
        let mut expected = Llsd::Integer(7);
        for _ in 0..16_u32 {
            expected = Llsd::Array(vec![expected]);
        }
        assert_eq!(parse_llsd_notation(bytes.as_bytes()), Ok(expected));
    }

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

    /// The writer emits the reference's spelling for each kind: bare digits
    /// for booleans, an `i`/`r`/`u` prefix on the numbers and the uuid,
    /// single-quoted strings, `b16` uppercase hex for binary, and sorted map
    /// keys.
    #[test]
    fn writes_the_reference_spelling() -> Result<(), TestError> {
        let spelling = |value: &Llsd| String::from_utf8(value.to_llsd_notation());
        assert_eq!(spelling(&Llsd::Undef)?, "!");
        assert_eq!(spelling(&Llsd::Boolean(true))?, "1");
        assert_eq!(spelling(&Llsd::Boolean(false))?, "0");
        assert_eq!(spelling(&Llsd::Integer(-42))?, "i-42");
        assert_eq!(spelling(&Llsd::Real(0.25))?, "r0.25");
        assert_eq!(spelling(&Llsd::String("hi".to_owned()))?, "'hi'");
        let uuid = "12345678-1234-1234-1234-1234567890ab";
        assert_eq!(
            spelling(&Llsd::Uuid(Uuid::parse_str(uuid)?))?,
            format!("u{uuid}")
        );
        assert_eq!(spelling(&Llsd::Binary(vec![0x0a, 0xff]))?, "b16\"0AFF\"");
        assert_eq!(
            spelling(&Llsd::Array(vec![Llsd::Integer(1), Llsd::Integer(2)]))?,
            "[i1,i2]"
        );
        let map = Llsd::Map(
            [
                ("z".to_owned(), Llsd::Integer(1)),
                ("a".to_owned(), Llsd::Integer(2)),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(spelling(&map)?, "{'a':i2,'z':i1}");
        Ok(())
    }

    /// A string is escaped the way the reference's table escapes it — named
    /// escapes for the control characters it names, `\xHH` for the rest and for
    /// every byte of a multi-byte UTF-8 sequence — and comes back as itself.
    #[test]
    fn writes_and_reads_back_escaped_strings() -> Result<(), TestError> {
        let awkward = "it's a\\b\nc\td\u{1}e\u{00e9}";
        let written = Llsd::String(awkward.to_owned()).to_llsd_notation();
        assert_eq!(
            String::from_utf8(written.clone())?,
            "'it\\'s a\\\\b\\nc\\td\\x01e\\xc3\\xa9'"
        );
        assert_eq!(
            parse_llsd_notation(&written)?,
            Llsd::String(awkward.to_owned())
        );
        Ok(())
    }

    /// Every kind survives writing and reading back, nested — so a document
    /// written here is one the parser (and the reference's) reads.
    #[test]
    fn every_kind_round_trips() -> Result<(), TestError> {
        let tree = Llsd::Map(
            [
                ("undef".to_owned(), Llsd::Undef),
                ("flag".to_owned(), Llsd::Boolean(true)),
                ("count".to_owned(), Llsd::Integer(-7)),
                ("real".to_owned(), Llsd::Real(1.0 / 3.0)),
                (
                    "id".to_owned(),
                    Llsd::Uuid(Uuid::from_u128(0x00C0_FFEE_u128)),
                ),
                (
                    "text".to_owned(),
                    Llsd::String("a 'quoted' \\ one".to_owned()),
                ),
                (
                    "uri".to_owned(),
                    Llsd::Uri("http://example/x?a=1".to_owned()),
                ),
                (
                    "when".to_owned(),
                    Llsd::Date("2026-09-01T12:00:00Z".to_owned()),
                ),
                ("blob".to_owned(), Llsd::Binary(vec![0, 1, 254, 255])),
                (
                    "list".to_owned(),
                    Llsd::Array(vec![Llsd::Real(-0.5), Llsd::Array(vec![Llsd::Undef])]),
                ),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(parse_llsd_notation(&tree.to_llsd_notation())?, tree);
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
