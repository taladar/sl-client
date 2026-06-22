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
    /// A scoped object/parcel id named a circuit that is no longer established
    /// (its circuit was torn down by a teleport, region crossing, relogin, or
    /// `DisableSimulator`). The id is stale and cannot be acted upon.
    #[error("the scoped id refers to a circuit that is no longer established")]
    UnknownCircuit,
    /// A batch object operation was given scoped ids belonging to more than one
    /// circuit; a single request targets exactly one simulator.
    #[error("the scoped ids belong to more than one circuit")]
    MixedCircuits,
}
