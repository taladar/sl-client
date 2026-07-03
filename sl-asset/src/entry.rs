//! The shared asset object the store hands out.
//!
//! An [`AssetEntry`] is held by consumers behind an `Arc`; the store keeps only
//! a `Weak` to it, so the asset becomes collectible once the last consumer drops
//! its `Arc` (pointer-count garbage collection). One entry represents one
//! logical asset: its class, its fetched bytes (once available), and its
//! observable progress. A generic asset is opaque — the store neither decodes it
//! nor tracks levels of detail — so the entry is a simple, lock-free byte holder
//! (reads go through `ArcSwap`) with a single-flight guard for the fetch.

use std::sync::Arc;

use arc_swap::{ArcSwap, ArcSwapOption};
use bytes::Bytes;
use sl_proto::{AssetKey, AssetType};

use crate::progress::AssetProgress;

/// One logical asset in the store: its class, its fetched bytes (if any), its
/// observable progress, and the lock serializing its fetch.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `AssetEntry` reads clearly"
)]
pub struct AssetEntry {
    /// The asset's id.
    pub(crate) id: AssetKey,
    /// The asset's class.
    pub(crate) asset_type: AssetType,
    /// The fetched asset bytes, or `None` before the fetch completes.
    pub(crate) data: ArcSwapOption<Bytes>,
    /// The current observable progress state.
    pub(crate) progress: ArcSwap<AssetProgress>,
    /// Signalled on every progress transition, to wake observers.
    pub(crate) progress_changed: event_listener::Event,
    /// Serializes the fetch (single-flight): concurrent gets for the same asset
    /// wait here and find the bytes already present.
    pub(crate) fetch_lock: async_lock::Mutex<()>,
}

impl std::fmt::Debug for AssetEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetEntry")
            .field("id", &self.id)
            .field("asset_type", &self.asset_type)
            .field("progress", &self.progress())
            .finish_non_exhaustive()
    }
}

impl AssetEntry {
    /// A fresh entry with no fetched bytes.
    pub(crate) fn new(id: AssetKey, asset_type: AssetType) -> Arc<Self> {
        Arc::new(Self {
            id,
            asset_type,
            data: ArcSwapOption::empty(),
            progress: ArcSwap::from_pointee(AssetProgress::Queued),
            progress_changed: event_listener::Event::new(),
            fetch_lock: async_lock::Mutex::new(()),
        })
    }

    /// The asset's id.
    #[must_use]
    pub const fn id(&self) -> AssetKey {
        self.id
    }

    /// The asset's class.
    #[must_use]
    pub const fn asset_type(&self) -> AssetType {
        self.asset_type
    }

    /// The fetched asset bytes, or `None` before the fetch completes. Cloning the
    /// returned [`Bytes`] is cheap (a refcount bump) and pins the bytes.
    #[must_use]
    pub fn data(&self) -> Option<Bytes> {
        self.data.load_full().map(|bytes| (*bytes).clone())
    }

    /// The current observable progress state.
    #[must_use]
    pub fn progress(&self) -> AssetProgress {
        **self.progress.load()
    }

    /// Publishes a new progress state and wakes observers if it changed.
    pub(crate) fn set_progress(&self, progress: AssetProgress) {
        if **self.progress.load() == progress {
            return;
        }
        self.progress.store(Arc::new(progress));
        let _notified = self.progress_changed.notify(usize::MAX);
    }

    /// Waits for the next progress transition.
    pub async fn progress_changed(&self) {
        self.progress_changed.listen().await;
    }
}
