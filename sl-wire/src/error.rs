//! Error type shared by the wire codec primitives.

use thiserror::Error;

use crate::message::MessageId;

/// An error encountered while decoding or encoding LLUDP wire data.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WireError {
    /// A datagram carried a message id with no corresponding known message.
    #[error("unrecognized message id {id:?}")]
    UnknownMessage {
        /// The unrecognized id.
        id: MessageId,
    },
    /// The reader ran out of bytes before a value could be fully read.
    #[error("unexpected end of data: needed {needed} more byte(s), had {available}")]
    UnexpectedEof {
        /// The number of additional bytes that were required.
        needed: usize,
        /// The number of bytes that were actually available.
        available: usize,
    },
    /// The datagram was too short to contain even a minimal packet header.
    #[error("datagram too short to contain a valid packet header")]
    ShortHeader,
    /// The appended acknowledgement list could not be read (count exceeds size).
    #[error("malformed appended acknowledgement list")]
    MalformedAcks,
    /// A zero-coded run was truncated (a `0x00` marker had no following count).
    #[error("truncated zero-coded data")]
    TruncatedZerocode,
    /// A variable-length value was longer than its length prefix can represent.
    #[error("variable-length value of {len} bytes exceeds the {max}-byte capacity")]
    VariableTooLong {
        /// The length of the offending value.
        len: usize,
        /// The maximum length representable by the prefix.
        max: usize,
    },
    /// A decoded field held a value outside the range its typed representation
    /// permits — for example a negative L$ amount in a field a conforming peer
    /// only ever sends non-negative, or an amount too large for its signed
    /// 32-bit wire slot. The message is rejected rather than silently coerced.
    #[error("field {field} carried out-of-range value {value}")]
    ValueOutOfRange {
        /// A short static label identifying the offending field.
        field: &'static str,
        /// The out-of-range value, rendered for diagnostics.
        value: i64,
    },
    /// A field that should carry a Second Life region name held a non-empty value
    /// that does not satisfy the region-name grammar (its length is outside the
    /// 2–35 character range the SL wiki documents). An empty value is the
    /// "unknown region" sentinel and decodes to `None`, not this error; only a
    /// non-empty but invalid name is rejected rather than silently coerced.
    #[error("field {field} carried invalid region name {value:?}")]
    InvalidRegionName {
        /// A short static label identifying the offending field.
        field: &'static str,
        /// The offending region name, rendered for diagnostics.
        value: String,
    },
    /// A field that should carry a UUID (often as text, e.g. an
    /// `EstateOwnerMessage` parameter or a string-encoded id) held a non-empty
    /// value that does not parse as one. An empty value, where the field treats
    /// it as an "absent" sentinel, decodes to `None` rather than this error;
    /// only a present-but-unparsable id is rejected rather than silently
    /// coerced to the nil UUID.
    #[error("field {field} carried invalid UUID {value:?}")]
    InvalidUuid {
        /// A short static label identifying the offending field.
        field: &'static str,
        /// The offending value, rendered for diagnostics.
        value: String,
    },
    /// A field that should carry a parseable scalar (e.g. an integer rendered as
    /// text in an `EstateOwnerMessage` parameter or a downloaded list file) held
    /// a value that could not be decoded. The message is rejected rather than
    /// silently coerced to a default (e.g. `0`).
    #[error("field {field} carried malformed value {value:?}")]
    MalformedField {
        /// A short static label identifying the offending field.
        field: &'static str,
        /// The offending value, rendered for diagnostics.
        value: String,
    },
    /// A field that a conforming peer is required to send was absent from a
    /// decoded body (an LLSD map key that should always be present, e.g. the
    /// identity id a capability reply is *about*). The message is rejected rather
    /// than silently substituting a default, since its absence means the body is
    /// malformed or from an incompatible peer.
    #[error("required field {field} was absent")]
    MissingField {
        /// A short static label identifying the absent field.
        field: &'static str,
    },
}
