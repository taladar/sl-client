//! The crate's error type.

/// Everything that can go wrong while starting or driving the fake grid.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Binding or using one of the loopback sockets failed.
    #[error("socket I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Serving an HTTP connection failed at the protocol level.
    #[error("HTTP connection failed: {0}")]
    Http(#[from] hyper::Error),
    /// A protocol-level send on a session failed (no circuit, encoding).
    #[error("session protocol error: {0}")]
    Proto(#[from] sl_proto::Error),
    /// A URL the grid mints for itself did not parse (a bug, not user input).
    #[error("minted URL did not parse: {0}")]
    Url(#[from] url::ParseError),
    /// The builder was asked to start a grid with no regions.
    #[error("a fake grid needs at least one region")]
    NoRegions,
    /// The builder was given a duplicate account or region name.
    #[error("duplicate {kind} {name:?}")]
    Duplicate {
        /// What collided ("account" or "region").
        kind: &'static str,
        /// The colliding name.
        name: String,
    },
    /// A teleport named a region the grid does not serve.
    #[error("unknown region {region:?}")]
    UnknownRegion {
        /// The region name (or index) that did not resolve.
        region: String,
    },
    /// A teleport was asked of a session whose agent has not arrived (a
    /// child circuit, or a login that never completed its movement).
    #[error("the agent is not the root agent of this session")]
    NotRootAgent,
    /// The client never completed its movement into the teleport
    /// destination; the source was told `timeout_tport` and the destination
    /// session was abandoned.
    #[error("the client did not arrive in the teleport destination in time")]
    TeleportTimedOut,
    /// A crossing named a region that does not border the one the agent is in
    /// (an avatar walks over a border; it does not walk across a grid).
    #[error("{to:?} is not a neighbour of {from:?}")]
    NotAdjacent {
        /// The region the agent is in.
        from: String,
        /// The region the crossing asked for.
        to: String,
    },
    /// The client never completed its movement into the region across the
    /// border; the agent stays the root agent of the region it was in.
    #[error("the client did not arrive across the border in time")]
    CrossingTimedOut,
    /// The session the teleport started from is not a registered account's
    /// (cannot happen for a session the grid minted).
    #[error("no account owns the teleporting agent")]
    UnknownAccount,
    /// An account referenced a start region the builder never defined.
    #[error("account {account:?} starts in undefined region {region:?}")]
    UnknownStartRegion {
        /// The account's `First Last` name.
        account: String,
        /// The missing region name.
        region: String,
    },
}
