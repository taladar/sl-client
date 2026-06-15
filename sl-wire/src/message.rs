//! The message-layer abstractions shared by every generated message type:
//! the frequency-coded [`MessageId`] and the [`Message`] trait implemented by
//! each generated message struct.

use crate::endian;
use crate::error::WireError;
use crate::field::{Reader, Writer};

/// The marker byte that introduces medium-, low-, and fixed-frequency ids.
const EXTEND: u8 = 0xFF;

/// A message identifier, encoded on the wire with a frequency-dependent prefix.
///
/// High ids are a single byte; medium ids are `0xFF` plus one byte; low ids are
/// `0xFF 0xFF` plus a big-endian `u16`; fixed ids are the full four-byte value
/// (always of the form `0xFFFFFFxx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageId {
    /// A high-frequency id (single byte).
    High(u8),
    /// A medium-frequency id (`0xFF` + one byte).
    Medium(u8),
    /// A low-frequency id (`0xFF 0xFF` + big-endian `u16`).
    Low(u16),
    /// A fixed id (the full four-byte value).
    Fixed(u32),
}

impl MessageId {
    /// Writes this id's frequency-coded prefix to `writer`.
    pub fn encode(self, writer: &mut Writer) {
        match self {
            Self::High(number) => writer.put_u8(number),
            Self::Medium(number) => {
                writer.put_u8(EXTEND);
                writer.put_u8(number);
            }
            Self::Low(number) => {
                writer.put_u8(EXTEND);
                writer.put_u8(EXTEND);
                writer.bytes(&endian::u16_to_be(number));
            }
            Self::Fixed(number) => writer.bytes(&endian::u32_to_be(number)),
        }
    }

    /// Reads a frequency-coded message id from `reader`.
    ///
    /// # Errors
    ///
    /// Returns [`WireError::UnexpectedEof`] if the id prefix is truncated.
    pub fn decode(reader: &mut Reader) -> Result<Self, WireError> {
        let first = reader.u8()?;
        if first != EXTEND {
            return Ok(Self::High(first));
        }
        let second = reader.u8()?;
        if second != EXTEND {
            return Ok(Self::Medium(second));
        }
        let high = reader.u8()?;
        let low = reader.u8()?;
        let value = endian::u16_from_be([high, low]);
        if high == EXTEND {
            // A fixed id of the form 0xFFFFFFxx.
            Ok(Self::Fixed(0xFFFF_0000 | u32::from(value)))
        } else {
            Ok(Self::Low(value))
        }
    }
}

/// A decodable, encodable LLUDP message body.
///
/// Implemented by every generated message struct. The associated constants
/// describe the message's identity and default encoding; the methods serialize
/// and deserialize only the message body (the blocks), not the packet header or
/// the frequency-coded id (which is handled by [`MessageId`]).
pub trait Message: Sized {
    /// The message name as it appears in the template (e.g. `UseCircuitCode`).
    const NAME: &'static str;
    /// The message's frequency-coded id.
    const ID: MessageId;
    /// Whether the message is zero-coded by default.
    const ZEROCODED: bool;

    /// Serializes the message body (its blocks) to `writer`.
    ///
    /// # Errors
    ///
    /// Returns a [`WireError`] if a variable-length value is too long to encode.
    fn encode_body(&self, writer: &mut Writer) -> Result<(), WireError>;

    /// Deserializes the message body (its blocks) from `reader`.
    ///
    /// # Errors
    ///
    /// Returns a [`WireError`] if the body is truncated or malformed.
    fn decode_body(reader: &mut Reader) -> Result<Self, WireError>;
}
