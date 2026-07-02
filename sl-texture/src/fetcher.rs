//! The runtime-agnostic network abstraction the store fetches codestream bytes
//! through.
//!
//! The store never speaks HTTP itself: each frontend (the tokio client with
//! async `reqwest`, the Bevy client with its blocking HTTP on a task pool)
//! supplies a [`TextureFetcher`] that fetches a byte range of a texture's
//! `GetTexture` codestream. Keeping this behind a trait is what lets the same
//! store core run under either executor.
//!
//! [`TextureFetcher`] is [`AssetFetcher`] keyed by [`TextureKey`]: it is a
//! blanket subtrait, so any `AssetFetcher<TextureKey>` *is* a `TextureFetcher`
//! with no extra code, and the store's fetch method
//! ([`fetch_range`](AssetFetcher::fetch_range)) comes straight from the shared
//! crate. [`FetchChunk`] and [`FetchError`] are re-exported from there unchanged.

use sl_proto::TextureKey;

#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `AssetFetcher` reads clearly"
)]
pub use sl_asset_sched::{AssetFetcher, FetchChunk, FetchError};

/// Fetches ranges of a texture's `GetTexture` codestream: an [`AssetFetcher`]
/// keyed by [`TextureKey`].
///
/// This is a blanket subtrait — every `AssetFetcher<TextureKey>` automatically
/// implements it — so a frontend implements [`AssetFetcher`] for `TextureKey`
/// (defining `fetch_range`) and gets `TextureFetcher` for free. The store stores
/// a `dyn TextureFetcher` and calls the inherited `fetch_range`.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `TextureFetcher` reads clearly"
)]
pub trait TextureFetcher: AssetFetcher<TextureKey> {}

impl<T: AssetFetcher<TextureKey> + ?Sized> TextureFetcher for T {}
