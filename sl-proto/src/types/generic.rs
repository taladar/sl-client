//! The generic method-name + parameter envelopes (`GenericMessage`,
//! `LargeGenericMessage`, `GenericStreamingMessage`).
//!
//! These are deliberately untyped carriers the simulator uses for many
//! loosely-coupled features: a method selector plus an opaque parameter
//! payload. The session surfaces them verbatim and leaves the
//! feature-specific parsing of [`params`](GenericMessage::params) /
//! [`data`](GenericStreamingMessage::data) to consumers.

use core::fmt;

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
///
/// Its `Debug` prints the method, then each parameter as a string when it is
/// UTF-8 (as bytes when it is not), then the invoice — so a log line reads
/// `method: "emptytrash", params: ["…"]` rather than a list of byte values,
/// and a test can find a message in printed output by its method and
/// parameters together.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GenericMessage {
    /// The method name selecting which feature this envelope carries.
    pub method: String,
    /// The feature-specific invoice id (a correlation id; often nil).
    pub invoice: InvoiceId,
    /// The opaque parameter blobs, in the order the simulator sent them.
    pub params: Vec<Vec<u8>>,
}

impl fmt::Debug for GenericMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GenericMessage")
            .field("method", &self.method)
            .field(
                "params",
                &DebugParams {
                    params: &self.params,
                },
            )
            .field("invoice", &self.invoice)
            .finish()
    }
}

/// A [`GenericMessage`]'s parameters, printed as strings where they are UTF-8.
struct DebugParams<'params> {
    /// The parameters.
    params: &'params [Vec<u8>],
}

impl fmt::Debug for DebugParams<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.params.iter().map(|param| DebugParam { param }))
            .finish()
    }
}

/// One parameter: a string when it is UTF-8, its bytes when it is not.
struct DebugParam<'param> {
    /// The parameter.
    param: &'param [u8],
}

impl fmt::Debug for DebugParam<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match core::str::from_utf8(self.param) {
            Ok(text) => fmt::Debug::fmt(text, f),
            Err(_not_utf8) => fmt::Debug::fmt(self.param, f),
        }
    }
}

/// An optimised generic envelope for streaming arbitrary data to the viewer,
/// parsed from a `GenericStreamingMessage`.
///
/// Unlike [`GenericMessage`], the method selector is a numeric
/// [`method`](Self::method) id (e.g.
/// [`GLTF_MATERIAL_OVERRIDE_METHOD`](sl_wire::GLTF_MATERIAL_OVERRIDE_METHOD))
/// and the payload is a single opaque [`data`](Self::data) blob (often
/// notation- or binary-encoded LLSD), kept verbatim for the consumer to decode.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GenericStreamingMessage {
    /// The numeric method id selecting which feature this envelope carries.
    pub method: u16,
    /// The opaque streamed payload.
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::GenericMessage;
    use crate::InvoiceId;

    /// A message prints its method and its UTF-8 parameters as strings, in
    /// that order, and a parameter that is not UTF-8 as its bytes.
    #[test]
    fn a_generic_message_prints_its_parameters_as_text() {
        let message = GenericMessage {
            method: "sl-fake-grid-marker".to_owned(),
            invoice: InvoiceId::default(),
            params: vec![b"arrived".to_vec(), vec![0xff, 0x00]],
        };
        let printed = format!("{message:?}");
        assert!(
            printed.starts_with(
                "GenericMessage { method: \"sl-fake-grid-marker\", params: [\"arrived\", [255, 0]], \
                 invoice: "
            ),
            "{printed}"
        );
        assert_eq!(printed.matches("arrived").count(), 1);
    }
}
