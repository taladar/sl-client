//! The runtime-agnostic network abstraction the store fetches asset bytes
//! through, and the typed key that names one generic asset.
//!
//! The store never speaks HTTP itself: each frontend (the tokio client with
//! async `reqwest`, the Bevy client with its blocking HTTP on a task pool)
//! supplies an [`AssetFetcher`] that fetches an asset over the `ViewerAsset`
//! capability. Keeping this behind a trait is what lets the same store core run
//! under either executor.
//!
//! Unlike a texture or mesh — each named by a single-class key ([`TextureKey`] /
//! [`MeshKey`]) — a generic asset's fetch URL is keyed by *both* its id and its
//! [`AssetType`] (the class picks the `?<class>_id=` query parameter). So the
//! store is keyed by [`AssetRef`], a `(id, class)` pair, and [`BlobFetcher`] is
//! [`AssetFetcher`] over that ref.
//!
//! [`FetchChunk`] and [`FetchError`] are re-exported from `sl-asset-sched`
//! unchanged.
//!
//! [`TextureKey`]: sl_proto::TextureKey
//! [`MeshKey`]: sl_proto::MeshKey

use sl_proto::{AssetKey, AssetType};

#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `AssetFetcher` reads clearly"
)]
pub use sl_asset_sched::{AssetFetcher, FetchChunk, FetchError};

/// A reference to one generic asset: its id and its [`AssetType`] class. The
/// class is part of the key because the `ViewerAsset` fetch URL selects the
/// asset by a class-specific query parameter (`?sound_id=`, `?bodypart_id=`, …),
/// so the same id fetched as a different class is a distinct request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AssetRef {
    /// The asset's id.
    pub id: AssetKey,
    /// The asset's class.
    pub asset_type: AssetType,
}

impl AssetRef {
    /// A ref naming asset `id` of class `asset_type`.
    #[must_use]
    pub const fn new(id: AssetKey, asset_type: AssetType) -> Self {
        Self { id, asset_type }
    }
}

impl std::fmt::Display for AssetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({:?})", self.id, self.asset_type)
    }
}

/// Fetches a generic asset's bytes over the `ViewerAsset` capability: an
/// [`AssetFetcher`] keyed by [`AssetRef`].
///
/// This is a blanket subtrait — every `AssetFetcher<AssetRef>` automatically
/// implements it — so a frontend implements [`AssetFetcher`] for `AssetRef`
/// (defining `fetch_range`) and gets `BlobFetcher` for free. The store stores a
/// `dyn BlobFetcher` and calls the inherited `fetch_range`.
///
/// Generic assets are opaque and fetched whole: the store requests the range
/// `0..usize::MAX`, which a fetcher treats as "the entire asset" (no `Range`
/// header).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `BlobFetcher` reads clearly"
)]
pub trait BlobFetcher: AssetFetcher<AssetRef> {}

impl<T: AssetFetcher<AssetRef> + ?Sized> BlobFetcher for T {}
