//! The generic method-name + parameter envelopes (`GenericMessage`,
//! `LargeGenericMessage`, `GenericStreamingMessage`).
//!
//! These are deliberately untyped carriers the simulator uses for many
//! loosely-coupled features: a method selector plus an opaque parameter
//! payload. The session surfaces them verbatim and leaves the
//! feature-specific parsing of [`params`](GenericMessage::params) /
//! [`data`](GenericStreamingMessage::data) to consumers.

use crate::InvoiceId;

/// A generic method-name + parameter-list envelope, parsed from a
/// `GenericMessage` (or its `LargeGenericMessage` analogue, which has the same
/// shape but a larger per-parameter size limit and, on real grids, an HTTP
/// transport).
///
/// The simulator uses this for a grab-bag of small features keyed by
/// [`method`](Self::method) (e.g. `"emptytrash"`, `"GrantUserRights"`); each
/// feature defines its own [`params`](Self::params) layout, so they are kept as
/// raw byte blobs here. In practice each parameter is a (usually
/// NUL-terminated) UTF-8 string, but the payload is preserved verbatim so a
/// consumer can decode it however the specific method requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericMessage {
    /// The method name selecting which feature this envelope carries.
    pub method: String,
    /// The feature-specific invoice id (a correlation id; often nil).
    pub invoice: InvoiceId,
    /// The opaque parameter blobs, in the order the simulator sent them.
    pub params: Vec<Vec<u8>>,
}

/// An optimised generic envelope for streaming arbitrary data to the viewer,
/// parsed from a `GenericStreamingMessage`.
///
/// Unlike [`GenericMessage`], the method selector is a numeric
/// [`method`](Self::method) id (e.g.
/// [`GLTF_MATERIAL_OVERRIDE_METHOD`](sl_wire::GLTF_MATERIAL_OVERRIDE_METHOD))
/// and the payload is a single opaque [`data`](Self::data) blob (often
/// notation- or binary-encoded LLSD), kept verbatim for the consumer to decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericStreamingMessage {
    /// The numeric method id selecting which feature this envelope carries.
    pub method: u16,
    /// The opaque streamed payload.
    pub data: Vec<u8>,
}
