//! Progress observation for an asset fetch.
//!
//! A generic asset is opaque and fetched whole in one shot, so — unlike a
//! texture (a progressive codestream) or a mesh (independent per-LOD blocks) —
//! there is no level-of-detail flow and no per-requester target to aggregate.
//! [`AssetProgress`] therefore reduces to the linear stages of a single fetch;
//! [`Priority`] is re-exported from `sl-asset-sched` for the store's admission
//! gate.

pub use sl_asset_sched::Priority;

/// The observable state of an asset fetch as it progresses.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `AssetProgress` reads clearly"
)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetProgress {
    /// Registered and awaiting a work slot behind the admission gate.
    Queued,
    /// Reading the cached bytes from the on-disk cache.
    ReadingDisk,
    /// Downloading the asset bytes over HTTP (`ViewerAsset`).
    Downloading {
        /// Bytes fetched so far.
        covered: usize,
    },
    /// Fetched and available, of the given byte length.
    Ready(usize),
    /// The fetch failed.
    Failed,
}
