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
    /// An account referenced a start region the builder never defined.
    #[error("account {account:?} starts in undefined region {region:?}")]
    UnknownStartRegion {
        /// The account's `First Last` name.
        account: String,
        /// The missing region name.
        region: String,
    },
}
