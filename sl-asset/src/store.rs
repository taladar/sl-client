//! The asset store: a weak-reference cache that fetches and keeps generic
//! assets, never fetching one twice while it is still referenced.
//!
//! A generic asset (sound, animation, landmark, notecard, gesture, body part,
//! clothing, …) is opaque and fetched whole over the `ViewerAsset` capability —
//! there is no progressive codestream and no level of detail, so the store's job
//! is caching and de-duplication, not decoding. It hands out `Arc<AssetEntry>`
//! and keeps only `Weak` references, so an asset is collectible once the last
//! external `Arc` drops. Concurrent requests for the same asset share one
//! fetch (single-flight) and one shared entry; an on-disk cache serves repeats
//! across runs.

use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry as MapEntry;
use sl_asset_sched::{Priority, PriorityGate};
use sl_proto::{AssetKey, AssetType};

use crate::disk::{AssetDiskCache, CacheLimits, now_unix};
use crate::entry::AssetEntry;
use crate::fetcher::{AssetRef, BlobFetcher, FetchError};
use crate::progress::AssetProgress;

/// Maximum number of assets fetching at once; the rest queue behind the gate.
const DEFAULT_INFLIGHT: usize = 16;

/// The admission priority the store fetches at. Generic assets have no
/// level-of-detail urgency to differentiate, so every fetch competes equally.
const FETCH_PRIORITY: Priority = Priority::new(1);

/// A failure to obtain a generic asset.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    /// The asset could not be fetched.
    #[error(transparent)]
    Fetch(#[from] FetchError),
}

/// The shared inner state of an [`AssetStore`].
#[derive(Debug)]
struct StoreInner {
    /// Live assets, held only weakly so pointer counts drive collection.
    map: DashMap<AssetRef, Weak<AssetEntry>>,
    /// The frontend-supplied network fetcher.
    fetcher: Arc<dyn BlobFetcher>,
    /// The optional on-disk cache (our own dedicated directory).
    disk: Option<AssetDiskCache>,
    /// The priority-ordered admission gate bounding in-flight fetches.
    gate: PriorityGate,
    /// Monotonic source of unique gate request ids.
    request_counter: AtomicU64,
}

/// A cloneable handle to a generic-asset fetch/cache store.
///
/// The store hands out `Arc<AssetEntry>`; it keeps only `Weak` references, so an
/// asset is collectible once the last external `Arc` drops. Requests for an
/// asset already in memory or in flight share it — an asset is never fetched
/// twice while referenced.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `AssetStore` reads clearly"
)]
#[derive(Clone, Debug)]
pub struct AssetStore(Arc<StoreInner>);

impl AssetStore {
    /// Builds a store over `fetcher`, optionally backed by an on-disk cache at
    /// `disk_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if `disk_dir` is given but its cache cannot be opened.
    pub fn new(
        fetcher: Arc<dyn BlobFetcher>,
        disk_dir: Option<std::path::PathBuf>,
        limits: CacheLimits,
    ) -> std::io::Result<Self> {
        let disk = match disk_dir {
            Some(dir) => Some(AssetDiskCache::open(dir, limits)?),
            None => None,
        };
        Ok(Self(Arc::new(StoreInner {
            map: DashMap::new(),
            fetcher,
            disk,
            gate: PriorityGate::new(DEFAULT_INFLIGHT),
            request_counter: AtomicU64::new(0),
        })))
    }

    /// Returns the live entry for `id` of class `asset_type` without fetching
    /// anything, or `None` if it is not currently in memory.
    #[must_use]
    pub fn peek(&self, id: AssetKey, asset_type: AssetType) -> Option<Arc<AssetEntry>> {
        self.0
            .map
            .get(&AssetRef::new(id, asset_type))
            .and_then(|reference| reference.value().upgrade())
    }

    /// Drops dead weak references from the map. Cheap; call periodically.
    pub fn sweep(&self) {
        self.0.map.retain(|_ref, weak| weak.strong_count() > 0);
    }

    /// Fetches `id`'s asset of class `asset_type`, returning the shared entry
    /// (from memory, the on-disk cache, or a fresh `ViewerAsset` download). A
    /// second `get` for a still-referenced asset returns the same `Arc` without
    /// re-fetching.
    ///
    /// # Errors
    ///
    /// Returns [`AssetError::Fetch`] if the asset is missing (`404`) or the
    /// download fails.
    pub async fn get(
        &self,
        id: AssetKey,
        asset_type: AssetType,
    ) -> Result<Arc<AssetEntry>, AssetError> {
        let asset_ref = AssetRef::new(id, asset_type);
        let entry = self.resolve_entry(asset_ref);

        // Fast path: already fetched and held in memory.
        if entry.data.load().is_some() {
            return Ok(entry);
        }

        // Single-flight: the first getter fetches; the rest wait here and find
        // the bytes already present when they take the lock.
        let guard = entry.fetch_lock.lock().await;
        if entry.data.load().is_some() {
            drop(guard);
            return Ok(entry);
        }

        // Bound concurrent fetches behind the shared admission gate.
        let request_id = self.0.request_counter.fetch_add(1, Ordering::Relaxed);
        let permit = self.0.gate.acquire(request_id, FETCH_PRIORITY).await;

        let bytes = self.load_bytes(&entry, asset_ref).await;
        drop(permit);

        match bytes {
            Ok(bytes) => {
                let len = bytes.len();
                entry.data.store(Some(Arc::new(bytes)));
                entry.set_progress(AssetProgress::Ready(len));
                drop(guard);
                Ok(entry)
            }
            Err(error) => {
                entry.set_progress(AssetProgress::Failed);
                drop(guard);
                Err(error)
            }
        }
    }

    /// Loads the asset bytes for `asset_ref`: the on-disk cache when it holds
    /// them, otherwise a whole-asset `ViewerAsset` download (which is written
    /// back to the cache on success).
    async fn load_bytes(
        &self,
        entry: &AssetEntry,
        asset_ref: AssetRef,
    ) -> Result<Bytes, AssetError> {
        if let Some(disk) = self.0.disk.as_ref() {
            entry.set_progress(AssetProgress::ReadingDisk);
            if let Some(bytes) = disk.read(asset_ref.id.uuid()) {
                return Ok(bytes);
            }
        }

        entry.set_progress(AssetProgress::Downloading { covered: 0 });
        // A generic asset is opaque and fetched whole: `0..usize::MAX` is the
        // fetcher's "entire asset" convention (it sends no `Range` header).
        let chunk = self.0.fetcher.fetch_range(asset_ref, 0, usize::MAX).await?;
        let bytes = chunk.bytes;

        if let Some(disk) = self.0.disk.as_ref()
            && let Err(error) = disk.write(asset_ref.id.uuid(), &bytes, now_unix())
        {
            tracing::debug!(%asset_ref, %error, "asset disk cache write failed");
        }
        Ok(bytes)
    }

    /// Returns the shared entry for `asset_ref`, creating (and weakly recording)
    /// a fresh one if none is currently live.
    fn resolve_entry(&self, asset_ref: AssetRef) -> Arc<AssetEntry> {
        match self.0.map.entry(asset_ref) {
            MapEntry::Occupied(mut occupied) => {
                if let Some(existing) = occupied.get().upgrade() {
                    return existing;
                }
                let fresh = AssetEntry::new(asset_ref.id, asset_ref.asset_type);
                occupied.insert(Arc::downgrade(&fresh));
                fresh
            }
            MapEntry::Vacant(vacant) => {
                let fresh = AssetEntry::new(asset_ref.id, asset_ref.asset_type);
                vacant.insert(Arc::downgrade(&fresh));
                fresh
            }
        }
    }
}
