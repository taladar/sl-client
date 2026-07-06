//! The runtime-agnostic network abstraction the store fetches codestream bytes
//! through, and the classification of *where* a texture is sourced from.
//!
//! The store never speaks HTTP itself: each frontend (the tokio client with
//! async `reqwest`, the Bevy client with its blocking HTTP on a task pool)
//! supplies a [`TextureFetcher`] that fetches a byte range of a texture's
//! codestream. Keeping this behind a trait is what lets the same store core run
//! under either executor.
//!
//! A texture's *source* mirrors the reference viewer's `FTType`: most textures
//! are fetched by UUID from the default asset service ([`FTT_DEFAULT`]), but a
//! server-side ("Sunshine") avatar bake is fetched from a wholly different
//! endpoint (the appearance service, [`FTT_SERVER_BAKE`]) and some textures are
//! produced locally and must never be fetched at all. That classification is not
//! carried on the wire — the reference viewer derives it from context when it
//! creates a fetch — so the caller names it here with a [`TextureFetchType`] and
//! narrows it to the fetchable [`RemoteTextureSource`] subset (rejecting the
//! local-only kinds up front) before handing it to the store.
//!
//! [`FTT_DEFAULT`]: TextureFetchType::Default
//! [`FTT_SERVER_BAKE`]: TextureFetchType::ServerBake

use async_trait::async_trait;
use sl_proto::TextureKey;
use thiserror::Error;

#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `AssetFetcher` reads clearly"
)]
pub use sl_asset_sched::{AssetFetcher, FetchChunk, FetchError};

/// Where a texture is sourced from, mirroring the reference viewer's `FTType`.
///
/// The full set includes kinds the client produces locally and must never fetch
/// over the network. Convert to a [`RemoteTextureSource`] (via [`TryFrom`]) to
/// fetch: that conversion rejects the local-only kinds, so a locally-generated
/// texture can never reach the [`TextureStore`](crate::TextureStore).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextureFetchType {
    /// A standard texture fetched by UUID from the default asset service
    /// (`GetTexture` / `ViewerAsset` CDN) — the reference `FTT_DEFAULT`.
    Default,
    /// A server-side ("Sunshine") avatar bake, fetched from the appearance service
    /// at this absolute URL (`<service>texture/<avatar>/<slot>/<uuid>`) — the
    /// reference `FTT_SERVER_BAKE`. A baked id is *not* fetchable by UUID from the
    /// CDN (which rejects it, typically with a `503`).
    ServerBake {
        /// The absolute appearance-service URL the bake is fetched from.
        url: String,
    },
    /// A texture the client generates locally and never fetches remotely: a local
    /// file (`FTT_LOCAL_FILE`) or a media-on-a-prim surface. Converting this to a
    /// [`RemoteTextureSource`] fails ([`NotRemotelyFetchable`]).
    Local,
}

/// The subset of [`TextureFetchType`] that names a network source the
/// [`TextureStore`](crate::TextureStore) can fetch from.
///
/// Obtained by [`TryFrom`]-ing a [`TextureFetchType`]; the local-only kinds error
/// at that boundary, so the store's fetch entry points only ever receive a source
/// that has a remote endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteTextureSource {
    /// Fetch by UUID from the default asset service (`GetTexture` / `ViewerAsset`).
    Default,
    /// Fetch from the appearance service at this absolute URL.
    ServerBake {
        /// The absolute appearance-service URL the bake is fetched from.
        url: String,
    },
}

/// The error from converting a locally-generated [`TextureFetchType`] into a
/// [`RemoteTextureSource`]: that kind has no network source, so it must be
/// produced by the client rather than fetched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("texture source is generated locally and has no remote endpoint to fetch")]
pub struct NotRemotelyFetchable;

impl TryFrom<TextureFetchType> for RemoteTextureSource {
    type Error = NotRemotelyFetchable;

    fn try_from(value: TextureFetchType) -> Result<Self, Self::Error> {
        match value {
            TextureFetchType::Default => Ok(Self::Default),
            TextureFetchType::ServerBake { url } => Ok(Self::ServerBake { url }),
            TextureFetchType::Local => Err(NotRemotelyFetchable),
        }
    }
}

impl From<RemoteTextureSource> for TextureFetchType {
    fn from(value: RemoteTextureSource) -> Self {
        match value {
            RemoteTextureSource::Default => Self::Default,
            RemoteTextureSource::ServerBake { url } => Self::ServerBake { url },
        }
    }
}

/// Fetches ranges of a texture's codestream from the texture's [`source`].
///
/// A frontend implements this over its own HTTP client (async `reqwest` for the
/// tokio runtime, blocking HTTP on a task pool for Bevy). The store stores a
/// `dyn TextureFetcher` and calls [`fetch_range`](Self::fetch_range), passing the
/// [`RemoteTextureSource`] the texture was requested with so the fetcher picks the
/// right endpoint (the default CDN by UUID, or a bake's appearance-service URL).
///
/// [`source`]: RemoteTextureSource
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `TextureFetcher` reads clearly"
)]
#[async_trait]
pub trait TextureFetcher: Send + Sync + std::fmt::Debug {
    /// Fetches the `[start, end)` byte range of `id`'s codestream from `source`.
    async fn fetch_range(
        &self,
        id: TextureKey,
        source: &RemoteTextureSource,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError>;
}

#[cfg(test)]
mod tests {
    use super::{NotRemotelyFetchable, RemoteTextureSource, TextureFetchType};
    use pretty_assertions::assert_eq;

    #[test]
    fn remote_kinds_convert() {
        assert_eq!(
            RemoteTextureSource::try_from(TextureFetchType::Default),
            Ok(RemoteTextureSource::Default)
        );
        let url = "https://appearance.example/".to_owned();
        assert_eq!(
            RemoteTextureSource::try_from(TextureFetchType::ServerBake { url: url.clone() }),
            Ok(RemoteTextureSource::ServerBake { url })
        );
    }

    #[test]
    fn local_kind_is_rejected_before_the_store() {
        assert_eq!(
            RemoteTextureSource::try_from(TextureFetchType::Local),
            Err(NotRemotelyFetchable)
        );
    }
}
