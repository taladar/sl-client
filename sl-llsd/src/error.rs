//! The error type returned by the [`Llsd`](crate::Llsd) typed field accessors.

/// An error reading a typed field out of an [`Llsd`](crate::Llsd) map.
///
/// The two variants mirror the way a CAPS/LLSD body can be malformed: a member
/// that should always be present is absent ([`MissingField`](Self::MissingField)),
/// or a present member carries a value of the wrong LLSD kind
/// ([`MalformedField`](Self::MalformedField)). Both are rejected rather than
/// silently coerced to a default, so a malformed body fails loudly. `sl-wire`
/// provides an `impl From<LlsdError> for WireError` that maps these straight onto
/// its own equivalents, so a `?` at a wire-side call site converts transparently.
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
}
