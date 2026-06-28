//! A minimal cursor over a notation-LLSD byte slice.
//!
//! Notation LLSD is the textual serialization Second Life / OpenSim use for some
//! payloads (e.g. the GLTF material-override `GenericStreamingMessage`). [`Scan`]
//! is sufficient to walk such a stream and slice out (without interpreting)
//! nested values; the domain decoders that interpret those values live in their
//! own crates.

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
