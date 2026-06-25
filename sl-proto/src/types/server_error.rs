//! Session error & forced-disconnect carriers (`Error`, `FeatureDisabled`,
//! `KickUser`).
//!
//! These are the messages a simulator sends when something the agent attempted
//! failed, a feature it asked for is unavailable, or the session is being torn
//! down from the server side. They surface as typed [`Event`](super::Event)s so
//! the application can react (display the message, abandon a pending request, or
//! observe the forced disconnect) instead of seeing them dropped to
//! `Diagnostic::UnhandledMessage`.

use sl_types::key::AgentKey;
use uuid::Uuid;

use crate::bookkeeping_ids::TransactionId;

/// A generic error the simulator (or a service behind it) reports over UDP,
/// parsed from an `Error` message.
///
/// This is the lowest-common-denominator error channel: at minimum a recipient
/// logs [`message`](Self::message), but a richer client can key off
/// [`code`](Self::code) (which mirrors HTTP status codes) and
/// [`system`](Self::system) (the hierarchical path to the originating handler,
/// e.g. `"message/handler"`) to react to a specific failure — for example
/// surfacing a money-transaction failure in the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerError {
    /// The agent the error is addressed to.
    pub agent: AgentKey,
    /// The error code; mirrors HTTP status codes.
    pub code: i32,
    /// A short machine-readable token identifying the error.
    pub token: String,
    /// The correlation id of whatever exchange failed. The wire field is a
    /// deliberately polymorphic "transaction id / unique id / session id /
    /// whatever", so it carries no single typed meaning and stays a raw
    /// [`Uuid`].
    pub id: Uuid,
    /// The hierarchical path to the originating system, e.g. `"message/handler"`.
    pub system: String,
    /// A human-readable description of the error.
    pub message: String,
    /// Extra info as binary-serialised LLSD, kept verbatim for the consumer to
    /// decode (empty when the sender supplied none).
    pub data: Vec<u8>,
}

/// A notice that a feature the agent requested is disabled, parsed from a
/// `FeatureDisabled` message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDisabled {
    /// A human-readable description of why the feature is unavailable.
    pub message: String,
    /// The agent the notice is addressed to.
    pub agent: AgentKey,
    /// The transaction the disabled feature would have served (often nil).
    pub transaction: TransactionId,
}

/// A server-initiated forced logout, parsed from a `KickUser` message — for
/// example when the same account logs in elsewhere.
///
/// The session also drives itself toward
/// [`Event::Disconnected`](super::Event::Disconnected) with
/// [`DisconnectReason::Kicked`](super::DisconnectReason::Kicked) when this
/// arrives, so observing either event is sufficient to learn the session ended.
/// The `KickUser` routing fields (the target sim's address and the agent's own
/// session id) carry nothing the application needs, so only the meaningful
/// payload is surfaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kick {
    /// The agent being kicked.
    pub agent: AgentKey,
    /// The human-readable reason for the kick.
    pub reason: String,
}
