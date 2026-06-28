//! The error type returned by the [`Llsd`](crate::Llsd) typed field accessors
//! and the binary-LLSD decoder.

/// An error reading an [`Llsd`](crate::Llsd) value — a typed map field of the
/// wrong kind or absent, or a malformed binary-LLSD byte stream.
///
/// The field variants mirror the way a CAPS/LLSD body can be malformed: a member
/// that should always be present is absent ([`MissingField`](Self::MissingField)),
/// or a present member carries a value of the wrong LLSD kind
/// ([`MalformedField`](Self::MalformedField)). Both are rejected rather than
/// silently coerced to a default, so a malformed body fails loudly. `sl-wire`
/// provides an `impl From<LlsdError> for WireError` that maps these straight onto
/// its own equivalents, so a `?` at a wire-side call site converts transparently.
///
/// The binary variants ([`TruncatedBinary`](Self::TruncatedBinary),
/// [`UnknownBinaryMarker`](Self::UnknownBinaryMarker),
/// [`MissingBinaryTerminator`](Self::MissingBinaryTerminator),
/// [`InvalidBinaryDate`](Self::InvalidBinaryDate)) are raised by
/// [`parse_llsd_binary`](crate::parse_llsd_binary) so a malformed cache file
/// fails cleanly instead of panicking on an out-of-bounds read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlsdError {
    /// A field that should carry a value of a particular LLSD kind held a value
    /// of a different kind. The message is rejected rather than silently coerced
    /// to a default.
    #[error("field {field} carried malformed value {value:?}")]
    MalformedField {
        /// A short static label identifying the offending field.
        field: &'static str,
        /// The offending value's LLSD kind, rendered for diagnostics.
        value: String,
    },
    /// A field that a conforming peer is required to send was absent from a
    /// decoded map. The message is rejected rather than silently substituting a
    /// default, since its absence means the body is malformed or from an
    /// incompatible peer.
    #[error("required field {field} was absent")]
    MissingField {
        /// A short static label identifying the absent field.
        field: &'static str,
    },
    /// A binary-LLSD stream ended before a complete value could be decoded — a
    /// truncated length/count prefix, a short scalar, or a string/binary body
    /// shorter than its declared length. Raised by
    /// [`parse_llsd_binary`](crate::parse_llsd_binary) rather than panicking on
    /// an out-of-bounds read.
    #[error("binary LLSD input ended unexpectedly")]
    TruncatedBinary,
    /// A binary-LLSD stream carried a type-marker byte that does not name any
    /// LLSD kind (nor a tolerated notation-string delimiter). Raised by
    /// [`parse_llsd_binary`](crate::parse_llsd_binary).
    #[error("unrecognized binary LLSD marker byte {marker:#04x}")]
    UnknownBinaryMarker {
        /// The offending marker byte.
        marker: u8,
    },
    /// A binary-LLSD array or map declared `count` entries (all of which were
    /// decoded) but was not closed by its mandatory `]` / `}` terminator byte.
    /// Firestorm's parser treats the terminator as mandatory, so
    /// [`parse_llsd_binary`](crate::parse_llsd_binary) does too.
    #[error("binary LLSD container missing its {expected:?} terminator")]
    MissingBinaryTerminator {
        /// The terminator byte that was required but absent (`]` or `}`).
        expected: char,
    },
    /// A binary-LLSD date carried an `f64` epoch-seconds value that does not map
    /// to a representable calendar timestamp. Raised by
    /// [`parse_llsd_binary`](crate::parse_llsd_binary).
    #[error("binary LLSD date is not a representable timestamp")]
    InvalidBinaryDate,
}
