//! The single error type of this crate's public parse surface.

use thiserror::Error;

use crate::message::MessageId;

/// An error encountered while decoding or encoding Second Life wire data.
///
/// This is the **one** failure type every `pub fn parse_*` in this crate
/// returns, whatever the encoding underneath: LLUDP datagrams, LLSD bodies,
/// XML-RPC calls and responses, the login handshake, and the capability bodies
/// that wrap zipped binary LLSD all fail into it. Nothing on the public surface
/// reports a fault as a bare `Option`/empty `Vec`, and no third-party error type
/// (notably the XML reader's — see [`Xml`](Self::Xml)) crosses the boundary, so
/// a caller writes one `match` and is not recompiled when a dependency of this
/// crate changes its error shape.
///
/// A `parse_*` that returns `Option`/`Ok(None)` is doing route matching, not
/// error reporting: "this URL suffix is not the one this endpoint serves", or
/// "this optional field is absent". Those are answers, not failures.
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
    /// A field that should carry a text-encoded scalar (an integer rendered as
    /// text in an `EstateOwnerMessage` parameter or a downloaded list-file line,
    /// e.g. a mute-list entry's type or flags) held a value that could not be
    /// decoded. The message is rejected rather than silently coerced to a default
    /// (e.g. `0`), matching the non-masking stance of
    /// [`InvalidUuid`](WireError::InvalidUuid). LLSD map-field faults are *not*
    /// reported here — those flow through [`Llsd`](WireError::Llsd) as an
    /// [`LlsdError`](sl_llsd::LlsdError) — so a text-scalar fault stays
    /// distinguishable from a structured-data one.
    #[error("field {field} carried invalid scalar {value:?}")]
    InvalidScalar {
        /// A short static label identifying the offending field.
        field: &'static str,
        /// The offending value, rendered for diagnostics.
        value: String,
    },
    /// A field that should carry a URL held a non-empty value that does not parse
    /// as one. An empty value, where the field treats it as an "absent" sentinel,
    /// decodes to `None` rather than this error; only a present-but-unparsable URL
    /// is rejected rather than silently coerced, matching the non-masking stance
    /// of [`InvalidRegionName`](WireError::InvalidRegionName) and
    /// [`InvalidUuid`](WireError::InvalidUuid).
    #[error("field {field} carried invalid URL {value:?}")]
    InvalidUrl {
        /// A short static label identifying the offending field.
        field: &'static str,
        /// The offending value, rendered for diagnostics.
        value: String,
    },
    /// An `LSLSyntax` document declared a `llsd-lsl-syntax-version` this decoder
    /// does not implement. The version is bumped only when the document's
    /// *schema* changes (not when the grid's function list changes), so an
    /// unrecognised value means the layout may differ from what
    /// [`parse_lsl_syntax`](crate::parse_lsl_syntax) expects — it is refused
    /// rather than parsed wrongly. An absent version key is reported the same way
    /// (rendered as `version: -1`), matching Firestorm's reject-on-missing stance.
    #[error("unsupported LSLSyntax document version {version} (expected {expected})")]
    UnsupportedLslSyntaxVersion {
        /// The version the document declared, or `-1` if the key was absent.
        version: i32,
        /// The single version this decoder implements.
        expected: i32,
    },
    /// A landmark asset body declared a `Landmark version` other than the two
    /// the reference viewer reads (1: global position, 2: region id + local
    /// position). Refused rather than guessed at, like
    /// [`UnsupportedLslSyntaxVersion`](WireError::UnsupportedLslSyntaxVersion).
    #[error("unsupported landmark version {version} (expected 1 or 2)")]
    UnsupportedLandmarkVersion {
        /// The version the body declared.
        version: u32,
    },
    /// An XML body was not well-formed.
    ///
    /// The XML reader's own diagnostic is **rendered into `message` rather than
    /// carried as its own type**: a caller that matches on this crate's failures
    /// then names nothing from the XML crate, and is neither recompiled nor
    /// broken when that dependency changes its error shape. The one distinction
    /// worth acting on — a body refused for depth rather than for syntax — has
    /// its own variant, [`XmlNestingTooDeep`](WireError::XmlNestingTooDeep).
    #[error("malformed XML: {message}")]
    Xml {
        /// The XML reader's diagnostic, rendered for humans.
        message: String,
    },
    /// An XML body nested arrays / elements deeper than
    /// [`MAX_NESTING_DEPTH`](sl_llsd::MAX_NESTING_DEPTH), and was refused by
    /// [`parse_guarded_xml`](sl_llsd::parse_guarded_xml) *before* the reader
    /// recursed into it — nesting depth is stack depth, and a stack overflow
    /// aborts the process rather than raising an error this type could carry.
    ///
    /// Distinct from [`Xml`](WireError::Xml) because it is not a claim that the
    /// document is malformed: a body this deep is well-formed and refused
    /// anyway, which is what a caller reporting "the grid sent something we
    /// will not read" needs to say.
    #[error("XML nested deeper than the {limit}-level limit")]
    XmlNestingTooDeep {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A document handed to an XML-RPC decoder was neither a `<methodCall>` nor
    /// a `<methodResponse>` — an HTML error page from a misrouted request, say.
    #[error("document is not an XML-RPC call or response")]
    NotXmlRpc,
    /// An XML-RPC `<methodCall>` carried no `<methodName>`.
    #[error("XML-RPC call has no methodName")]
    NoMethodName,
    /// A typed XML-RPC decoder was handed a response for a different method, or
    /// a call naming a method it does not implement. The document is refused
    /// rather than decoded against the wrong field layout.
    #[error("unexpected XML-RPC method {method:?}")]
    UnexpectedMethod {
        /// The method name found.
        method: String,
    },
    /// The peer answered an XML-RPC call with a `<fault>` instead of a result —
    /// how a login host reports a refused login.
    #[error("XML-RPC fault: {message}")]
    XmlRpcFault {
        /// The `faultString` member, or `"unknown fault"` when the fault struct
        /// carried none.
        message: String,
    },
    /// An XML-RPC `<methodResponse>` carried no `<struct>` where the decoder
    /// requires one — a well-formed document that is not the reply shape.
    #[error("XML-RPC document carries no response struct")]
    NoStruct,
    /// A compressed capability body — the `{ "Zipped": … }` envelope the legacy
    /// `RenderMaterials` capability carries — did not inflate, or inflated past
    /// the decoder's size limit. Refused rather than read as an absent body, so
    /// a corrupt reply is distinguishable from the empty "fetch everything"
    /// request that legitimately carries no envelope.
    #[error("compressed {what} body could not be inflated")]
    MalformedCompressedBody {
        /// A short static label naming the body that failed to inflate.
        what: &'static str,
    },
    /// A fault decoding a [`Llsd`](sl_llsd::Llsd) body: a map field read by the
    /// typed `field_*` / `require_*` accessors was absent or of the wrong LLSD
    /// kind, or an LLSD-XML document failed to parse. This wraps the LLSD core's
    /// own [`LlsdError`](sl_llsd::LlsdError) so that a structured-data fault stays
    /// **distinguishable** from the text-scalar
    /// [`InvalidScalar`](WireError::InvalidScalar) / [`InvalidUuid`](WireError::InvalidUuid)
    /// faults a non-LLSD parser (XML-RPC login, scalar list-file fields, …) raises
    /// directly.
    #[error(transparent)]
    Llsd(#[from] sl_llsd::LlsdError),
}

impl From<roxmltree::Error> for WireError {
    /// Wraps the XML reader's error, keeping the one distinction a caller acts
    /// on — a body refused by the nesting guard rather than for bad syntax —
    /// and rendering the rest, so no `roxmltree` type reaches the public
    /// surface. `parse_guarded_xml` reports its own depth refusal as
    /// `NodesLimitReached`, which is why that variant maps to
    /// [`XmlNestingTooDeep`](WireError::XmlNestingTooDeep).
    fn from(error: roxmltree::Error) -> Self {
        match error {
            roxmltree::Error::NodesLimitReached => Self::XmlNestingTooDeep {
                limit: sl_llsd::MAX_NESTING_DEPTH,
            },
            other => Self::Xml {
                message: other.to_string(),
            },
        }
    }
}
