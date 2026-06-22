//! LLUDP packet framing: the fixed packet header, flag bits, appended
//! acknowledgements, and datagram (de)framing.
//!
//! Every LLUDP datagram begins with a six-byte prelude: a flags byte, a
//! four-byte big-endian sequence number, and a one-byte extra-header length
//! followed by that many extra-header bytes. When the [`PacketFlags::ACK`] bit
//! is set, the datagram ends with a list of big-endian `u32` acknowledgement
//! ids and a final one-byte count. When [`PacketFlags::ZEROCODED`] is set, the
//! message body (everything between the extra header and the appended acks) is
//! zero-coded (see [`crate::zerocode`]).

use crate::endian;
use crate::error::WireError;
use crate::sequence_number::SequenceNumber;

/// The number of bytes in the fixed part of the header (flags, sequence,
/// extra-header length), before any extra-header bytes.
const PRELUDE_LEN: usize = 6;

/// The LLUDP packet flag bits carried in the first header byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketFlags {
    /// The raw flags byte.
    bits: u8,
}

impl PacketFlags {
    /// No flags set.
    pub const EMPTY: Self = Self { bits: 0x00 };
    /// The body is zero-coded.
    pub const ZEROCODED: Self = Self { bits: 0x80 };
    /// The packet is sent reliably and must be acknowledged.
    pub const RELIABLE: Self = Self { bits: 0x40 };
    /// The packet is a retransmission of an earlier one.
    pub const RESENT: Self = Self { bits: 0x20 };
    /// The packet carries appended acknowledgements.
    pub const ACK: Self = Self { bits: 0x10 };

    /// Builds flags from a raw byte.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    /// Returns the raw flags byte.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.bits
    }

    /// Returns `true` if every bit in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.bits & other.bits == other.bits
    }

    /// Returns a copy of `self` with the bits in `other` also set.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

/// A datagram parsed into its prelude, appended acks, and (still possibly
/// zero-coded) message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDatagram<'a> {
    /// The packet flags.
    pub flags: PacketFlags,
    /// The big-endian sequence number.
    pub sequence: SequenceNumber,
    /// Any extra-header bytes (usually empty).
    pub extra: &'a [u8],
    /// Acknowledgement ids appended to this datagram (already stripped from the
    /// body), in wire order.
    pub acks: Vec<SequenceNumber>,
    /// The message body. Still zero-coded if `flags.contains(PacketFlags::ZEROCODED)`.
    pub body: &'a [u8],
}

/// Parses a raw datagram into its prelude, appended acks, and message body.
///
/// Appended acknowledgements are stripped from the end first (the final byte is
/// the ack count, preceding it are that many big-endian `u32` ids); the
/// returned `body` is everything between the extra header and the acks, and is
/// still zero-coded when the [`PacketFlags::ZEROCODED`] bit is set.
///
/// # Errors
///
/// Returns [`WireError::ShortHeader`] if the datagram is too short for a header
/// and [`WireError::MalformedAcks`] if the appended ack list is inconsistent.
pub fn parse_datagram(datagram: &[u8]) -> Result<ParsedDatagram<'_>, WireError> {
    let flags_byte = datagram.first().ok_or(WireError::ShortHeader)?;
    let flags = PacketFlags::from_bits(*flags_byte);

    let seq_bytes: [u8; 4] = datagram
        .get(1..5)
        .ok_or(WireError::ShortHeader)?
        .try_into()
        .map_err(|_ignored| WireError::ShortHeader)?;
    let sequence = SequenceNumber(endian::u32_from_be(seq_bytes));

    let extra_len = usize::from(*datagram.get(5).ok_or(WireError::ShortHeader)?);
    let extra_end = PRELUDE_LEN
        .checked_add(extra_len)
        .ok_or(WireError::ShortHeader)?;
    let extra = datagram
        .get(PRELUDE_LEN..extra_end)
        .ok_or(WireError::ShortHeader)?;

    let mut end = datagram.len();
    let mut acks = Vec::new();
    if flags.contains(PacketFlags::ACK) {
        let count_index = end.checked_sub(1).ok_or(WireError::MalformedAcks)?;
        let count = usize::from(*datagram.get(count_index).ok_or(WireError::MalformedAcks)?);
        let acks_len = count.checked_mul(4).ok_or(WireError::MalformedAcks)?;
        let acks_start = count_index
            .checked_sub(acks_len)
            .ok_or(WireError::MalformedAcks)?;
        if acks_start < extra_end {
            return Err(WireError::MalformedAcks);
        }
        let acks_slice = datagram
            .get(acks_start..count_index)
            .ok_or(WireError::MalformedAcks)?;
        acks.reserve(count);
        for chunk in acks_slice.chunks_exact(4) {
            let id_bytes: [u8; 4] = chunk
                .try_into()
                .map_err(|_ignored| WireError::MalformedAcks)?;
            acks.push(SequenceNumber(endian::u32_from_be(id_bytes)));
        }
        end = acks_start;
    }

    let body = datagram.get(extra_end..end).ok_or(WireError::ShortHeader)?;
    Ok(ParsedDatagram {
        flags,
        sequence,
        extra,
        acks,
        body,
    })
}

/// Frames a message `body` into a datagram with the given `flags` and
/// `sequence`, an empty extra header, and no appended acknowledgements.
///
/// The caller is responsible for the consistency of `flags` with `body` (for
/// example, only setting [`PacketFlags::ZEROCODED`] when `body` is zero-coded).
#[must_use]
pub fn encode_datagram(flags: PacketFlags, sequence: SequenceNumber, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(PRELUDE_LEN.saturating_add(body.len()));
    out.push(flags.bits());
    out.extend_from_slice(&endian::u32_to_be(sequence.get()));
    out.push(0x00); // extra-header length
    out.extend_from_slice(body);
    out
}
