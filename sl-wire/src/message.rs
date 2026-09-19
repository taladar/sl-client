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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// What `message_template.msg` says about a message's standing over LLUDP.
///
/// The template marks 26 of its messages: 5 `Deprecated`, 17 `UDPDeprecated`
/// and 4 `UDPBlackListed`. Every one of them is still code-generated — a
/// simulator may well send it, and OpenSim sends several of the blacklisted
/// ones routinely — but the flag is the wire-level record of *why* the client
/// should not be reaching for it, so it travels with the generated type rather
/// than being discarded at parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MessageStatus {
    /// No deprecation flag: an ordinary, current message.
    #[default]
    Current,
    /// `Deprecated`: obsolete on every transport.
    Deprecated,
    /// `UDPDeprecated`: obsolete over LLUDP. Second Life carries these over the
    /// CAPS event queue instead (`ParcelProperties`, `ScriptRunningReply`,
    /// `LargeGenericMessage`, …), though OpenSim still sends some over UDP.
    UdpDeprecated,
    /// `UDPBlackListed`: refused over LLUDP (`TeleportFinish`, `CrossedRegion`,
    /// `EnableSimulator`, `OpenCircuit`).
    UdpBlackListed,
}

impl MessageStatus {
    /// The template flag this status came from, or `None` for
    /// [`Self::Current`] — the spelling used in `message_template.msg`.
    #[must_use]
    pub const fn flag(self) -> Option<&'static str> {
        match self {
            Self::Current => None,
            Self::Deprecated => Some("Deprecated"),
            Self::UdpDeprecated => Some("UDPDeprecated"),
            Self::UdpBlackListed => Some("UDPBlackListed"),
        }
    }

    /// Whether the template marks this message obsolete (`Deprecated` or
    /// `UDPDeprecated`). A blacklisted message is *refused*, not obsolete, so
    /// it answers `false` here — test [`Self::is_udp_blacklisted`] for that.
    #[must_use]
    pub const fn is_deprecated(self) -> bool {
        matches!(self, Self::Deprecated | Self::UdpDeprecated)
    }

    /// Whether the template blacklists this message over LLUDP.
    #[must_use]
    pub const fn is_udp_blacklisted(self) -> bool {
        matches!(self, Self::UdpBlackListed)
    }
}

impl core::fmt::Display for MessageStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.flag().unwrap_or("Current"))
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
    /// What the template's trailing flags say about the message's standing.
    const STATUS: MessageStatus;

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
