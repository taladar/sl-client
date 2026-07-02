//! The runtime-agnostic network abstraction the store fetches codestream bytes
//! through.
//!
//! The store never speaks HTTP itself: each frontend (the tokio client with
//! async `reqwest`, the Bevy client with its blocking HTTP on a task pool)
//! supplies a [`TextureFetcher`] that fetches a byte range of a texture's
//! `GetTexture` codestream. Keeping this behind a trait is what lets the same
//! store core run under either executor.

use bytes::Bytes;
use sl_proto::TextureKey;

/// The result of fetching a codestream byte range.
#[derive(Clone, Debug)]
pub struct FetchChunk {
    /// The returned bytes: the requested gap on a `206 Partial Content`, or the
    /// entire asset when the server ignored the range and answered `200`.
    pub bytes: Bytes,
    /// Whether `bytes` is the whole asset (a `200` response), so the store should
    /// replace rather than append and mark the codestream complete.
    pub whole: bool,
}

/// A texture fetch failure.
#[derive(Clone, Debug, thiserror::Error)]
pub enum FetchError {
    /// The texture does not exist (a `404`, the fetch equivalent of not found).
    #[error("texture not found")]
    NotFound,
    /// A transport-level failure (connection, timeout, malformed response).
    #[error("texture fetch failed: {0}")]
    Transport(String),
}

/// Fetches ranges of a texture's `GetTexture` codestream. Implemented per
/// frontend; the store calls it to grow a codestream toward a level of detail.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `TextureFetcher` reads clearly"
)]
#[async_trait::async_trait]
pub trait TextureFetcher: Send + Sync + std::fmt::Debug {
    /// Fetches the byte range `start..end` of `id`'s codestream. A conforming
    /// server answers `206` with exactly that range; one that ignores the range
    /// answers `200` with the whole asset (signalled by [`FetchChunk::whole`]).
    ///
    /// # Errors
    ///
    /// Returns [`FetchError::NotFound`] for a missing texture, or
    /// [`FetchError::Transport`] for a network/protocol failure.
    async fn fetch_range(
        &self,
        id: TextureKey,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError>;
}
