//! Public value types of the sans-I/O session: its inputs and outputs.

use std::net::SocketAddr;

use sl_wire::LoginRequest;

/// The parameters needed to start a session: where to log in and with what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginParams {
    /// The XML-RPC login endpoint URL (e.g. `http://127.0.0.1:9000/`).
    pub login_uri: String,
    /// The login request to send.
    pub request: LoginRequest,
}

/// An HTTP request the driver must perform on the session's behalf: POST `body`
/// to `url` and feed the response back via
/// [`Session::handle_login_response`](crate::Session::handle_login_response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginHttpRequest {
    /// The URL to POST to.
    pub url: String,
    /// The XML-RPC request body.
    pub body: String,
}

/// How an outgoing message should be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reliability {
    /// Send once, best-effort.
    Unreliable,
    /// Send reliably: track acknowledgement and retransmit until acked.
    Reliable,
}

/// A datagram ready to be sent on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transmit {
    /// Where to send the datagram.
    pub destination: SocketAddr,
    /// The datagram bytes.
    pub payload: Vec<u8>,
}

/// Why a session became disconnected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// The login server rejected the credentials.
    LoginFailed {
        /// The machine-readable reason code.
        reason: String,
        /// The human-readable message.
        message: String,
    },
    /// No traffic was received within the inactivity budget.
    Timeout,
    /// A reliable handshake packet exhausted its retransmissions.
    HandshakeFailed,
    /// An unrecoverable wire-protocol error occurred.
    ProtocolError,
}

/// A high-level event surfaced to the driver/application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The UDP circuit to the simulator has been opened and the bootstrap
    /// packets queued.
    CircuitEstablished {
        /// The simulator's UDP address.
        sim: SocketAddr,
    },
    /// The region handshake completed; the session is now fully active.
    RegionHandshakeComplete,
    /// The session logged out cleanly (a `LogoutReply` was received).
    LoggedOut,
    /// The session disconnected for the given reason.
    Disconnected(DisconnectReason),
}
