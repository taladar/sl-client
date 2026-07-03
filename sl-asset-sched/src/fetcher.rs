//! The runtime-agnostic network abstraction the stores fetch asset bytes
//! through.
//!
//! A store never speaks HTTP itself: each frontend (the tokio client with async
//! `reqwest`, the Bevy client with its blocking HTTP on a task pool) supplies an
//! [`AssetFetcher`] that fetches a byte range of an asset. Keeping this behind a
//! trait is what lets the same store core run under either executor. The trait
//! is generic over the asset key `K` (a typed key such as `TextureKey` or
//! `MeshKey`), so each store fetches its own asset class without a shared key
//! type.

use bytes::Bytes;

/// The result of fetching an asset byte range.
#[derive(Clone, Debug)]
pub struct FetchChunk {
    /// The returned bytes: the requested gap on a `206 Partial Content`, or the
    /// entire asset when the server ignored the range and answered `200`.
    pub bytes: Bytes,
    /// Whether `bytes` is the whole asset (a `200` response), so the store should
    /// replace rather than append and mark the fetch complete.
    pub whole: bool,
}

/// An asset fetch failure.
#[derive(Clone, Debug, thiserror::Error)]
pub enum FetchError {
    /// The asset does not exist (a `404`, the fetch equivalent of not found).
    #[error("asset not found")]
    NotFound,
    /// The asset service is unavailable: it kept answering with a transient
    /// status (a `503`, and behind a proxy `502` / `504`) until the fetcher's
    /// retries were exhausted. Distinct from [`Transport`](Self::Transport) so a
    /// caller can treat a persistently-unavailable service as a soft failure. The
    /// payload is the server's last response (status line + body snippet), so the
    /// caller can log *why* the service was unavailable.
    #[error("asset service unavailable (retries exhausted): {0}")]
    Unavailable(String),
    /// A transport-level failure: a connection/timeout/protocol error, or a
    /// non-success HTTP status. The payload describes it (for an HTTP status, the
    /// status line plus a body snippet).
    #[error("asset fetch failed: {0}")]
    Transport(String),
}

/// Fetches ranges of an asset over HTTP. Implemented per frontend; a store calls
/// it to grow an asset toward a level of detail. Generic over the asset key `K`.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `AssetFetcher` reads clearly"
)]
#[async_trait::async_trait]
pub trait AssetFetcher<K>: Send + Sync + std::fmt::Debug {
    /// Fetches the byte range `start..end` of `id`'s asset. A conforming server
    /// answers `206` with exactly that range; one that ignores the range answers
    /// `200` with the whole asset (signalled by [`FetchChunk::whole`]).
    ///
    /// # Errors
    ///
    /// Returns [`FetchError::NotFound`] for a missing asset, or
    /// [`FetchError::Transport`] for a network/protocol failure.
    async fn fetch_range(&self, id: K, start: usize, end: usize) -> Result<FetchChunk, FetchError>;
}
