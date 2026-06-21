//! Error type for the sans-I/O session.

use thiserror::Error;

/// An error returned by a [`Session`](crate::Session) input method.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// A wire-level (de)serialization error.
    #[error("wire error: {0}")]
    Wire(#[from] sl_wire::WireError),
    /// An operation requiring an established circuit was attempted before login
    /// completed.
    #[error("the session has no active circuit yet")]
    NoCircuit,
    /// A teleport was requested while the session was not in the active state
    /// (for example before the region handshake, or during another teleport).
    #[error("the session is not active")]
    NotActive,
}
