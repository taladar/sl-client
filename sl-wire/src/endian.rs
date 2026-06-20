//! Centralized byte-order conversions for the LLUDP wire format.
//!
//! The LLUDP protocol mixes byte orders: the packet sequence number, the
//! frequency-coded message id, and appended acknowledgement ids are big-endian
//! (network byte order), while message field payloads are little-endian. The
//! `big_endian_bytes` and `little_endian_bytes` clippy lints are denied
//! crate-wide, so every `to_*_bytes`/`from_*_bytes` conversion is confined to
//! the two submodules below, each carrying a single localized expectation.

pub(crate) use big::{
    f64_from_be, f64_to_be, i32_from_be, i32_to_be, u16_from_be, u16_to_be, u32_from_be, u32_to_be,
    u64_from_be, u64_to_be,
};
pub(crate) use little::{
    f32_from_le, f32_to_le, f64_from_le, f64_to_le, i16_from_le, i16_to_le, i32_from_le, i32_to_le,
    u16_from_le, u16_to_le, u32_from_le, u32_to_le, u64_from_le, u64_to_le,
};

/// Big-endian conversions: sequence numbers, message ids and appended acks.
mod big {
    #![expect(
        clippy::big_endian_bytes,
        reason = "LLUDP sequence numbers, message ids and appended acks are wire-defined big-endian"
    )]

    /// Reads a big-endian `u32` from four bytes.
    pub(crate) const fn u32_from_be(bytes: [u8; 4]) -> u32 {
        u32::from_be_bytes(bytes)
    }

    /// Writes a `u32` as four big-endian bytes.
    pub(crate) const fn u32_to_be(value: u32) -> [u8; 4] {
        value.to_be_bytes()
    }

    /// Reads a big-endian `u16` from two bytes.
    pub(crate) const fn u16_from_be(bytes: [u8; 2]) -> u16 {
        u16::from_be_bytes(bytes)
    }

    /// Writes a `u16` as two big-endian bytes.
    pub(crate) const fn u16_to_be(value: u16) -> [u8; 2] {
        value.to_be_bytes()
    }

    /// Reads a big-endian `i32` from four bytes (binary-LLSD integers).
    pub(crate) const fn i32_from_be(bytes: [u8; 4]) -> i32 {
        i32::from_be_bytes(bytes)
    }

    /// Writes an `i32` as four big-endian bytes (binary-LLSD integers).
    pub(crate) const fn i32_to_be(value: i32) -> [u8; 4] {
        value.to_be_bytes()
    }

    /// Reads a big-endian `u64` from eight bytes (binary-LLSD region handles).
    pub(crate) const fn u64_from_be(bytes: [u8; 8]) -> u64 {
        u64::from_be_bytes(bytes)
    }

    /// Writes a `u64` as eight big-endian bytes (binary-LLSD region handles).
    pub(crate) const fn u64_to_be(value: u64) -> [u8; 8] {
        value.to_be_bytes()
    }

    /// Reads a big-endian `f64` from eight bytes (binary-LLSD reals).
    pub(crate) const fn f64_from_be(bytes: [u8; 8]) -> f64 {
        f64::from_be_bytes(bytes)
    }

    /// Writes an `f64` as eight big-endian bytes (binary-LLSD reals).
    pub(crate) const fn f64_to_be(value: f64) -> [u8; 8] {
        value.to_be_bytes()
    }
}

/// Little-endian conversions: all message field payloads.
mod little {
    #![expect(
        clippy::little_endian_bytes,
        reason = "LLUDP message field payloads are wire-defined little-endian"
    )]

    /// Reads a little-endian `u16` from two bytes.
    pub(crate) const fn u16_from_le(bytes: [u8; 2]) -> u16 {
        u16::from_le_bytes(bytes)
    }

    /// Writes a `u16` as two little-endian bytes.
    pub(crate) const fn u16_to_le(value: u16) -> [u8; 2] {
        value.to_le_bytes()
    }

    /// Reads a little-endian `u32` from four bytes.
    pub(crate) const fn u32_from_le(bytes: [u8; 4]) -> u32 {
        u32::from_le_bytes(bytes)
    }

    /// Writes a `u32` as four little-endian bytes.
    pub(crate) const fn u32_to_le(value: u32) -> [u8; 4] {
        value.to_le_bytes()
    }

    /// Reads a little-endian `u64` from eight bytes.
    pub(crate) const fn u64_from_le(bytes: [u8; 8]) -> u64 {
        u64::from_le_bytes(bytes)
    }

    /// Writes a `u64` as eight little-endian bytes.
    pub(crate) const fn u64_to_le(value: u64) -> [u8; 8] {
        value.to_le_bytes()
    }

    /// Reads a little-endian `i16` from two bytes.
    pub(crate) const fn i16_from_le(bytes: [u8; 2]) -> i16 {
        i16::from_le_bytes(bytes)
    }

    /// Writes an `i16` as two little-endian bytes.
    pub(crate) const fn i16_to_le(value: i16) -> [u8; 2] {
        value.to_le_bytes()
    }

    /// Reads a little-endian `i32` from four bytes.
    pub(crate) const fn i32_from_le(bytes: [u8; 4]) -> i32 {
        i32::from_le_bytes(bytes)
    }

    /// Writes an `i32` as four little-endian bytes.
    pub(crate) const fn i32_to_le(value: i32) -> [u8; 4] {
        value.to_le_bytes()
    }

    /// Reads a little-endian `f32` from four bytes.
    pub(crate) const fn f32_from_le(bytes: [u8; 4]) -> f32 {
        f32::from_le_bytes(bytes)
    }

    /// Writes an `f32` as four little-endian bytes.
    pub(crate) const fn f32_to_le(value: f32) -> [u8; 4] {
        value.to_le_bytes()
    }

    /// Reads a little-endian `f64` from eight bytes.
    pub(crate) const fn f64_from_le(bytes: [u8; 8]) -> f64 {
        f64::from_le_bytes(bytes)
    }

    /// Writes an `f64` as eight little-endian bytes.
    pub(crate) const fn f64_to_le(value: f64) -> [u8; 8] {
        value.to_le_bytes()
    }
}
