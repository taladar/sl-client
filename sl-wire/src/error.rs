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
}
