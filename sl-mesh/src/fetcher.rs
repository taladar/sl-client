//! The runtime-agnostic network abstraction the store fetches mesh bytes
//! through.
//!
//! The store never speaks HTTP itself: each frontend (the tokio client with
//! async `reqwest`, the Bevy client with its blocking HTTP on a task pool)
//! supplies a [`MeshFetcher`] that fetches a byte range of a mesh's
//! `GetMesh2` / `GetMesh` asset. Keeping this behind a trait is what lets the
//! same store core run under either executor.
//!
//! [`MeshFetcher`] is [`AssetFetcher`] keyed by [`MeshKey`]: it is a blanket
//! subtrait, so any `AssetFetcher<MeshKey>` *is* a `MeshFetcher` with no extra
//! code, and the store's fetch method ([`fetch_range`](AssetFetcher::fetch_range))
//! comes straight from the shared crate. [`FetchChunk`] and [`FetchError`] are
//! re-exported from there unchanged.

use sl_proto::MeshKey;

#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `AssetFetcher` reads clearly"
)]
pub use sl_asset_sched::{AssetFetcher, FetchChunk, FetchError};

/// Fetches ranges of a mesh's `GetMesh2` / `GetMesh` asset: an [`AssetFetcher`]
/// keyed by [`MeshKey`].
///
/// This is a blanket subtrait — every `AssetFetcher<MeshKey>` automatically
/// implements it — so a frontend implements [`AssetFetcher`] for `MeshKey`
/// (defining `fetch_range`) and gets `MeshFetcher` for free. The store stores a
/// `dyn MeshFetcher` and calls the inherited `fetch_range`.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `MeshFetcher` reads clearly"
)]
pub trait MeshFetcher: AssetFetcher<MeshKey> {}

impl<T: AssetFetcher<MeshKey> + ?Sized> MeshFetcher for T {}
