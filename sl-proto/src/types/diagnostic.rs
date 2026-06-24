//! Protocol-level diagnostics: anomalies the session noticed in inbound data.

use sl_wire::{MessageId, SequenceNumber, WireError};

/// A protocol-level anomaly the session noticed while processing inbound data.
///
/// Diagnostics are kept strictly **separate** from [`Event`](crate::Event): a
/// match on `Event` never sees a diagnostic, and vice versa. Where an `Event`
/// is a successfully understood happening a client acts on, a `Diagnostic`
/// surfaces something the session would otherwise *silently drop* — a datagram
/// whose body failed to decode, a decoded message with no handler, an unknown
/// or malformed CAPS event-queue payload, or a reliable request whose expected
/// reply never arrived. They exist so a test client (or a developer chasing a
/// protocol gap) can see exactly what the session is ignoring.
///
/// Collection is **off by default** — diagnostics are produced only after
/// [`Session::set_diagnostics(true)`](crate::Session::set_diagnostics), so the
/// raw-byte capture and bookkeeping cost nothing on the normal path. Drain them
/// with [`Session::poll_diagnostic`](crate::Session::poll_diagnostic).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Diagnostic {
    /// An inbound datagram carried a message whose id was recognised (or at
    /// least frequency-decodable) but whose body failed to decode. The session
    /// drops such datagrams; this captures what was lost.
    DecodeFailed {
        /// The frequency-coded id read from the datagram.
        id: MessageId,
        /// The message name, when `id` maps to a known template message
        /// (`None` for an unrecognised id).
        name: Option<&'static str>,
        /// The wire error that decoding produced.
        error: WireError,
        /// The decoded message body (post zero-decode), captured for a hexdump.
        /// Only populated while diagnostics are enabled.
        raw: Vec<u8>,
        /// The reader offset into `raw` at which decoding stopped — the byte to
        /// mark in a hexdump.
        failed_offset: usize,
    },
    /// A message decoded successfully but reached the dispatch table's
    /// catch-all arm: nothing in the session acts on it. (Expected for traffic
    /// the client does not model; useful to know which messages those are.)
    UnhandledMessage {
        /// The message's frequency-coded id.
        id: MessageId,
        /// The message name.
        name: &'static str,
        /// Whether it arrived on a child-agent circuit (a neighbouring region)
        /// rather than the root circuit.
        child: bool,
    },
    /// A CAPS event-queue event (or capability reply) arrived under a name the
    /// session does not handle.
    UnknownCapsEvent {
        /// The event / capability name as delivered.
        message: String,
    },
    /// A CAPS event the session *does* handle arrived, but its LLSD body failed
    /// to parse into the expected shape (a required field was absent, a field
    /// held the wrong LLSD kind, or a legacy `from_llsd` returned `None`).
    CapsDecodeFailed {
        /// The event / capability name whose body could not be parsed.
        message: String,
        /// The decode error that caused the drop, rendered for debugging (which
        /// field was missing or malformed). [`None`] for the legacy
        /// `Option`-returning decoders that do not report a specific cause.
        reason: Option<String>,
    },
    /// A reliable request never received its expected reply: either a reliable
    /// packet exhausted its retransmission budget, or an operation awaiting a
    /// reply (logout, sit) timed out. (Teleport timeouts stay
    /// [`Event::TeleportFailed`](crate::Event::TeleportFailed) instead.)
    ExpectedReplyMissing {
        /// A short label for the request whose reply is missing (e.g. the
        /// reliable message name, or `"Logout"` / `"Sit"`).
        request: String,
        /// The sequence number of the unacked reliable packet, when one is
        /// known (`None` for operation-level timeouts).
        sequence: Option<SequenceNumber>,
    },
}
