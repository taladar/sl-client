//! Typed, bounds-checked cursors for reading and writing LLUDP message fields.
//!
//! Every read is checked against the remaining buffer and returns a
//! [`WireError`] on underflow rather than panicking, satisfying the crate's
//! no-panic, no-indexing lint policy. All multi-byte integers and floats are
//! little-endian (see [`crate::endian`]); UUIDs are 16 raw bytes.

use sl_types::lsl::{Rotation, Vector};
use uuid::Uuid;

use crate::endian;
use crate::error::WireError;

/// A cursor reading typed values from a byte slice.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    /// The backing buffer.
    buf: &'a [u8],
    /// The current read offset into `buf`.
    pos: usize,
    /// Whether a fixed-size read (and a variable field's payload) past the end
    /// of `buf` yields zero bytes instead of an error. See
    /// [`Reader::with_zero_tail`].
    zero_tail: bool,
    /// How many bytes past the end of `buf` were read as zeros.
    zero_filled: usize,
}

impl<'a> Reader<'a> {
    /// Creates a reader over `buf`, positioned at its start.
    #[must_use]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            zero_tail: false,
            zero_filled: 0,
        }
    }

    /// Creates a reader over `buf` whose reads past its end yield **zero
    /// bytes** rather than [`WireError::UnexpectedEof`].
    ///
    /// This is how the reference `LLTemplateMessageReader` decodes a message
    /// that runs off the end of its packet: it logs "Ran off end of packet" and
    /// zero-fills the field. Second Life's simulators depend on it — they send
    /// zero-coded messages whose final run of zeros is encoded short (observed
    /// on `ObjectUpdate`: usually an encoded body ending `00 42`, 66 zeros where
    /// the block's trailing fields need 67, leaving its last `F32` three bytes
    /// long; once a message came up 37 bytes short). Nothing follows that run,
    /// so what it under-counts is zeros, and zero-filling decodes the message
    /// just as the reference does.
    ///
    /// Callers enable it only where that cause applies (see
    /// [`crate::ends_in_zero_run`]) and report
    /// [`Reader::zero_filled`] when it is non-zero, so a message decoded this
    /// way is never silent. [`Reader::take`] and the other slice-returning
    /// reads stay strict: they cannot lend bytes that do not exist.
    #[must_use]
    pub const fn with_zero_tail(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            zero_tail: true,
            zero_filled: 0,
        }
    }

    /// How many bytes past the end of the buffer have been read as zeros (only
    /// ever non-zero for a [`Reader::with_zero_tail`] reader).
    #[must_use]
    pub const fn zero_filled(&self) -> usize {
        self.zero_filled
    }

    /// Returns the number of bytes not yet consumed.
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// Returns the current read offset into the backing buffer.
    ///
    /// After a failed read this is the offset at which decoding stopped, which
    /// callers can surface as a `failed_offset` for diagnostics.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// Returns `true` if no bytes remain to be read.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Consumes and returns the next `n` bytes.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than `n` bytes remain.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| WireError::UnexpectedEof {
                needed: n,
                available: self.remaining(),
            })?;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| WireError::UnexpectedEof {
                needed: n,
                available: self.remaining(),
            })?;
        self.pos = end;
        Ok(slice)
    }

    /// Consumes and returns the remaining bytes.
    pub fn take_rest(&mut self) -> &'a [u8] {
        let rest = self.buf.get(self.pos..).unwrap_or(&[]);
        self.pos = self.buf.len();
        rest
    }

    /// Returns the not-yet-consumed bytes without advancing the reader.
    ///
    /// Useful for measuring a self-delimiting sub-structure (e.g. an
    /// `ExtraParams` container) before consuming it with [`take`](Self::take).
    #[must_use]
    pub fn peek_rest(&self) -> &'a [u8] {
        self.buf.get(self.pos..).unwrap_or(&[])
    }

    /// Consumes the next `N` bytes as a fixed-size array.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than `N` bytes remain.
    pub fn take_array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        if self.zero_tail && self.remaining() < N {
            let mut array = [0u8; N];
            let available = self.peek_rest();
            for (slot, byte) in array.iter_mut().zip(available) {
                *slot = *byte;
            }
            self.zero_filled = self
                .zero_filled
                .saturating_add(N.saturating_sub(available.len()));
            self.pos = self.buf.len();
            return Ok(array);
        }
        let slice = self.take(N)?;
        slice
            .try_into()
            .map_err(|_ignored| WireError::UnexpectedEof {
                needed: N,
                available: slice.len(),
            })
    }

    /// Reads the one-byte repeat count of a template `Variable` block, or `0`
    /// when the data has already ended.
    ///
    /// A message may omit trailing `Variable` blocks entirely (OpenSim's shorter
    /// `RegionInfo`; the reference reader calls it legal), so the missing count
    /// is neither an error nor a [`Reader::zero_filled`] byte.
    pub fn variable_block_count(&mut self) -> u8 {
        if self.is_empty() {
            return 0;
        }
        self.u8().unwrap_or(0)
    }

    /// Reads `len` bytes into an owned buffer, zero-filling whatever lies past
    /// the end of the data for a [`Reader::with_zero_tail`] reader.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than `len` bytes remain
    /// and the reader is not zero-tailed.
    fn take_owned(&mut self, len: usize) -> Result<Vec<u8>, WireError> {
        if self.zero_tail && self.remaining() < len {
            let mut bytes = self.take_rest().to_vec();
            self.zero_filled = self
                .zero_filled
                .saturating_add(len.saturating_sub(bytes.len()));
            bytes.resize(len, 0);
            return Ok(bytes);
        }
        Ok(self.take(len)?.to_vec())
    }

    /// Reads a one-byte-length-prefixed variable field into an owned buffer —
    /// the template decoder's form of [`Reader::variable1`], which also honours
    /// [`Reader::with_zero_tail`].
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if the prefix or payload is short
    /// and the reader is not zero-tailed.
    pub fn variable1_owned(&mut self) -> Result<Vec<u8>, WireError> {
        let len = usize::from(self.u8()?);
        self.take_owned(len)
    }

    /// Reads a two-byte-length-prefixed variable field into an owned buffer —
    /// the template decoder's form of [`Reader::variable2`], which also honours
    /// [`Reader::with_zero_tail`].
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if the prefix or payload is short
    /// and the reader is not zero-tailed.
    pub fn variable2_owned(&mut self) -> Result<Vec<u8>, WireError> {
        let len = usize::from(self.u16()?);
        self.take_owned(len)
    }

    /// Reads a `u8`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if no bytes remain.
    pub fn u8(&mut self) -> Result<u8, WireError> {
        let [byte] = self.take_array::<1>()?;
        Ok(byte)
    }

    /// Reads a one-byte boolean (any non-zero byte is `true`).
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if no bytes remain.
    pub fn bool(&mut self) -> Result<bool, WireError> {
        Ok(self.u8()? != 0)
    }

    /// Reads an `i8`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if no bytes remain.
    pub fn i8(&mut self) -> Result<i8, WireError> {
        // Single byte: native byte order is identical to little-endian and
        // avoids the (denied) explicit little-endian conversion lint.
        Ok(i8::from_ne_bytes(self.take_array::<1>()?))
    }

    /// Reads a little-endian `u16`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than two bytes remain.
    pub fn u16(&mut self) -> Result<u16, WireError> {
        Ok(endian::u16_from_le(self.take_array::<2>()?))
    }

    /// Reads a little-endian `i16`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than two bytes remain.
    pub fn i16(&mut self) -> Result<i16, WireError> {
        Ok(endian::i16_from_le(self.take_array::<2>()?))
    }

    /// Reads a little-endian `u32`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than four bytes remain.
    pub fn u32(&mut self) -> Result<u32, WireError> {
        Ok(endian::u32_from_le(self.take_array::<4>()?))
    }

    /// Reads a little-endian `i32`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than four bytes remain.
    pub fn i32(&mut self) -> Result<i32, WireError> {
        Ok(endian::i32_from_le(self.take_array::<4>()?))
    }

    /// Reads a little-endian `u64`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than eight bytes remain.
    pub fn u64(&mut self) -> Result<u64, WireError> {
        Ok(endian::u64_from_le(self.take_array::<8>()?))
    }

    /// Reads a little-endian `f32`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than four bytes remain.
    pub fn f32(&mut self) -> Result<f32, WireError> {
        Ok(endian::f32_from_le(self.take_array::<4>()?))
    }

    /// Reads a little-endian `f64`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than eight bytes remain.
    pub fn f64(&mut self) -> Result<f64, WireError> {
        Ok(endian::f64_from_le(self.take_array::<8>()?))
    }

    /// Reads a 16-byte UUID.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than 16 bytes remain.
    pub fn uuid(&mut self) -> Result<Uuid, WireError> {
        Ok(Uuid::from_bytes(self.take_array::<16>()?))
    }

    /// Reads an `LLVector3` (three little-endian floats).
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than 12 bytes remain.
    pub fn vector3(&mut self) -> Result<Vector, WireError> {
        let x = self.f32()?;
        let y = self.f32()?;
        let z = self.f32()?;
        Ok(Vector { x, y, z })
    }

    /// Reads an `LLQuaternion`, sent as three little-endian floats with the
    /// `s` component reconstructed from the unit-length constraint.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than 12 bytes remain.
    pub fn quaternion(&mut self) -> Result<Rotation, WireError> {
        let x = self.f32()?;
        let y = self.f32()?;
        let z = self.f32()?;
        Ok(Rotation {
            x,
            y,
            z,
            s: reconstruct_w(x, y, z),
        })
    }

    /// Reads an `LLVector3d` (three little-endian doubles).
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than 24 bytes remain.
    pub fn vector3d(&mut self) -> Result<[f64; 3], WireError> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }

    /// Reads an `LLVector4` (four little-endian floats).
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if fewer than 16 bytes remain.
    pub fn vector4(&mut self) -> Result<[f32; 4], WireError> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }

    /// Reads a variable-length byte string prefixed by a one-byte length.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if the prefix or payload is short.
    pub fn variable1(&mut self) -> Result<&'a [u8], WireError> {
        let len = usize::from(self.u8()?);
        self.take(len)
    }

    /// Reads a variable-length byte string prefixed by a little-endian
    /// two-byte length.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if the prefix or payload is short.
    pub fn variable2(&mut self) -> Result<&'a [u8], WireError> {
        let len = usize::from(self.u16()?);
        self.take(len)
    }
}

/// Reconstructs the `s` component of a normalized quaternion from `x`, `y`, `z`.
fn reconstruct_w(x: f32, y: f32, z: f32) -> f32 {
    let sum_of_squares = x.mul_add(x, y.mul_add(y, z * z));
    (1.0_f32 - sum_of_squares).max(0.0_f32).sqrt()
}

/// A growable buffer for writing typed values in little-endian wire form.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Writer {
    /// The accumulated output bytes.
    buf: Vec<u8>,
}

impl Writer {
    /// Creates an empty writer.
    #[must_use]
    pub const fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Consumes the writer, returning the accumulated bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Returns the bytes written so far.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Appends raw bytes verbatim.
    pub fn bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Writes a `u8`.
    pub fn put_u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    /// Writes a one-byte boolean.
    pub fn put_bool(&mut self, value: bool) {
        self.buf.push(u8::from(value));
    }

    /// Writes an `i8`.
    pub fn put_i8(&mut self, value: i8) {
        self.buf.extend_from_slice(&value.to_ne_bytes());
    }

    /// Writes a little-endian `u16`.
    pub fn put_u16(&mut self, value: u16) {
        self.buf.extend_from_slice(&endian::u16_to_le(value));
    }

    /// Writes a little-endian `i16`.
    pub fn put_i16(&mut self, value: i16) {
        self.buf.extend_from_slice(&endian::i16_to_le(value));
    }

    /// Writes a little-endian `u32`.
    pub fn put_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&endian::u32_to_le(value));
    }

    /// Writes a little-endian `i32`.
    pub fn put_i32(&mut self, value: i32) {
        self.buf.extend_from_slice(&endian::i32_to_le(value));
    }

    /// Writes a little-endian `u64`.
    pub fn put_u64(&mut self, value: u64) {
        self.buf.extend_from_slice(&endian::u64_to_le(value));
    }

    /// Writes a little-endian `f32`.
    pub fn put_f32(&mut self, value: f32) {
        self.buf.extend_from_slice(&endian::f32_to_le(value));
    }

    /// Writes a little-endian `f64`.
    pub fn put_f64(&mut self, value: f64) {
        self.buf.extend_from_slice(&endian::f64_to_le(value));
    }

    /// Writes a 16-byte UUID.
    pub fn put_uuid(&mut self, value: Uuid) {
        self.buf.extend_from_slice(value.as_bytes());
    }

    /// Writes an `LLVector3` (three little-endian floats).
    pub fn put_vector3(&mut self, value: &Vector) {
        self.put_f32(value.x);
        self.put_f32(value.y);
        self.put_f32(value.z);
    }

    /// Writes an `LLQuaternion` as three little-endian floats (the `s`
    /// component is dropped and reconstructed by the receiver).
    pub fn put_quaternion(&mut self, value: &Rotation) {
        self.put_f32(value.x);
        self.put_f32(value.y);
        self.put_f32(value.z);
    }

    /// Writes an `LLVector3d` (three little-endian doubles).
    pub fn put_vector3d(&mut self, value: [f64; 3]) {
        let [x, y, z] = value;
        self.put_f64(x);
        self.put_f64(y);
        self.put_f64(z);
    }

    /// Writes an `LLVector4` (four little-endian floats).
    pub fn put_vector4(&mut self, value: [f32; 4]) {
        let [x, y, z, w] = value;
        self.put_f32(x);
        self.put_f32(y);
        self.put_f32(z);
        self.put_f32(w);
    }

    /// Writes a variable-length byte string prefixed by a one-byte length.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::VariableTooLong`] if `data` is longer than 255.
    pub fn put_variable1(&mut self, data: &[u8]) -> Result<(), WireError> {
        let len = u8::try_from(data.len()).map_err(|_ignored| WireError::VariableTooLong {
            len: data.len(),
            max: usize::from(u8::MAX),
        })?;
        self.buf.push(len);
        self.buf.extend_from_slice(data);
        Ok(())
    }

    /// Writes a variable-length byte string prefixed by a little-endian
    /// two-byte length.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::VariableTooLong`] if `data` is longer than 65535.
    pub fn put_variable2(&mut self, data: &[u8]) -> Result<(), WireError> {
        let len = u16::try_from(data.len()).map_err(|_ignored| WireError::VariableTooLong {
            len: data.len(),
            max: usize::from(u16::MAX),
        })?;
        self.buf.extend_from_slice(&endian::u16_to_le(len));
        self.buf.extend_from_slice(data);
        Ok(())
    }
}
