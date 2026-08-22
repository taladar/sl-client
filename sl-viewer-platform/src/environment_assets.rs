//! EEP environment-settings **asset** fetch / decode / cache — the data source
//! for the World ▸ Environment **Modern** menu presets.
//!
//! The reference viewer's fixed-time presets load specific `AT_SETTINGS` library
//! assets by UUID (`LLEnvironment::KNOWN_SKY_*`) through the same rendering
//! pipeline as any other sky. Fetching those exact assets here and decoding them
//! ([`environment_asset_from_bytes`]) lets the viewer render byte-identical input
//! to Firestorm, so a renderer comparison isolates the renderer rather than the
//! sky data.
//!
//! The fetch reuses the generic [`AssetStore`] over the `ViewerAsset` capability —
//! the same infrastructure the animation / wearable fetches use — with
//! [`AssetType::Settings`]. Mirrors
//! `AnimationManager`, minus the
//! built-in / local-file paths a settings asset never has.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::{
    AssetCacheLimits, AssetKey, AssetStore, AssetType, BevyAssetFetcher, BlobFetcher,
    CAP_VIEWER_ASSET, EnvironmentAsset, GateStats, SlCapabilities, StoreStats,
    environment_asset_from_bytes,
};

/// The EEP settings-asset resolve / decode / cache pipeline: an [`AssetStore`]
/// over the `ViewerAsset` capability fetching `AT_SETTINGS` assets by UUID, the
/// in-flight fetch+decode tasks, the decoded [`EnvironmentAsset`]s already in
/// hand, and the ids known to have no fetchable/decodable asset.
///
/// Backs the World ▸ Environment **Modern** presets (the `KNOWN_SKY_*` library
/// skies). Mirrors `AnimationManager`.
#[derive(Debug, Resource)]
pub struct EnvironmentAssetManager {
    /// The generic-asset store doing the `ViewerAsset` fetch, dedupe, off-thread
    /// work, and on-disk caching of the raw settings bytes.
    store: AssetStore,
    /// The store's HTTP fetcher, kept so its `ViewerAsset` capability URL can be
    /// refreshed on a region change.
    fetcher: Arc<BevyAssetFetcher>,
    /// The background fetch+decode task per settings id, polled to completion by
    /// [`poll_environment_assets`]; presence means "already being resolved".
    inflight: HashMap<AssetKey, Task<Option<EnvironmentAsset>>>,
    /// Successfully decoded settings by id, shared so an asset is fetched and
    /// decoded once however many times its preset is (re)selected.
    decoded: HashMap<AssetKey, Arc<EnvironmentAsset>>,
    /// Ids whose fetch or decode failed, so [`request`](Self::request) does not
    /// retry them forever.
    unavailable: HashSet<AssetKey>,
    /// Ids requested before the region's `ViewerAsset` capability was known, held
    /// here so the fetch is not run — and the id not marked permanently
    /// [`unavailable`](Self::unavailable) — until the cap arrives. Drained by
    /// `retry_pending`.
    pending: HashSet<AssetKey>,
}

impl Default for EnvironmentAssetManager {
    fn default() -> Self {
        let fetcher = Arc::new(BevyAssetFetcher::new());
        let store = build_settings_store(&fetcher, settings_cache_dir());
        Self {
            store,
            fetcher,
            inflight: HashMap::new(),
            decoded: HashMap::new(),
            unavailable: HashSet::new(),
            pending: HashSet::new(),
        }
    }
}

impl EnvironmentAssetManager {
    /// Ensure the settings asset `id` is being resolved: a nil id, an
    /// already-decoded id, one in flight, or one known unavailable is ignored. If
    /// the `ViewerAsset` capability is not known yet the request is held pending
    /// (not fetched, not marked unavailable) until it arrives. Idempotent.
    pub fn request(&mut self, id: AssetKey) {
        if id.uuid().is_nil()
            || self.decoded.contains_key(&id)
            || self.inflight.contains_key(&id)
            || self.unavailable.contains(&id)
        {
            return;
        }
        if !self.fetcher.has_cap_url() {
            let _inserted = self.pending.insert(id);
            return;
        }
        let _removed = self.pending.remove(&id);
        debug!("resolving environment settings asset {}", id.uuid());
        let store = self.store.clone();
        // Both the HTTP fetch and the LLSD decode run on this IoTaskPool thread, so
        // the render thread never touches settings bytes.
        let task = IoTaskPool::get().spawn(async move {
            let bytes = match store.get(id, AssetType::Settings).await {
                Ok(entry) => match entry.data() {
                    Some(data) => data.to_vec(),
                    None => {
                        warn!("settings asset {} fetched but has no data", id.uuid());
                        return None;
                    }
                },
                Err(error) => {
                    warn!(
                        "fetching settings asset {} over ViewerAsset: {error}",
                        id.uuid()
                    );
                    return None;
                }
            };
            match environment_asset_from_bytes(&id.uuid().to_string(), &bytes) {
                Some(asset) => Some(asset),
                None => {
                    warn!(
                        "decoding settings asset {}: not a sky/water settings map",
                        id.uuid()
                    );
                    None
                }
            }
        });
        let _prev = self.inflight.insert(id, task);
    }

    /// The decoded settings for `id`, once resolved, or `None` while it is still
    /// in flight, failed, or was never requested.
    #[must_use]
    pub fn get(&self, id: AssetKey) -> Option<&Arc<EnvironmentAsset>> {
        self.decoded.get(&id)
    }

    /// A point-in-time snapshot of the settings-asset fetch/decode pipeline, for
    /// the F3 diagnostics overlay: entry counts bucketed by stage plus the
    /// cumulative disk-cache-hit / GC counters. Delegates to the wrapped
    /// [`AssetStore`].
    #[must_use]
    pub fn stats(&self) -> StoreStats {
        self.store.stats()
    }

    /// A point-in-time snapshot of the settings store's admission gate: its
    /// concurrency capacity, in-flight slots, and queued waiters.
    #[must_use]
    pub fn gate_stats(&self) -> GateStats {
        self.store.gate_stats()
    }

    /// How many resolves are parked outside the store's own accounting — held for
    /// the `ViewerAsset` capability that is not up yet (see
    /// `pending`) — so the pipeline overlay does not report
    /// "nothing left to load" while such work is still outstanding.
    #[must_use]
    pub fn deferred_count(&self) -> usize {
        self.pending.len()
    }

    /// Point the store's fetcher at the region's current `ViewerAsset` URL.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
    }

    /// Re-issue any settings resolves parked before the `ViewerAsset` capability
    /// was known (see `pending`), now that it is. A no-op while
    /// the cap is unset or nothing is pending.
    fn retry_pending(&mut self) {
        if self.pending.is_empty() || !self.fetcher.has_cap_url() {
            return;
        }
        let pending: Vec<AssetKey> = self.pending.drain().collect();
        for id in pending {
            self.request(id);
        }
    }

    /// Re-park every settings asset previously marked
    /// [`unavailable`](Self::unavailable) so the next
    /// `retry_pending` re-fetches it. Called on a capability
    /// refresh (a region cross / reconnect): a settings asset whose fetch failed
    /// transiently would otherwise keep its fallback for the session.
    fn rearm_unavailable(&mut self) {
        if self.unavailable.is_empty() {
            return;
        }
        let failed: Vec<AssetKey> = self.unavailable.drain().collect();
        for id in failed {
            let _inserted = self.pending.insert(id);
        }
    }
}

/// Build an [`AssetStore`] over `fetcher`, disk-backed when the cache opens and
/// in-memory only otherwise (a cache failure must never wedge the viewer).
/// Mirrors the animation-asset store builder.
fn build_settings_store(fetcher: &Arc<BevyAssetFetcher>, disk_dir: Option<PathBuf>) -> AssetStore {
    let concrete = Arc::clone(fetcher);
    let fetcher: Arc<dyn BlobFetcher> = concrete;
    if let Some(dir) = disk_dir {
        match AssetStore::new(
            Arc::clone(&fetcher),
            Some(dir),
            AssetCacheLimits {
                max_bytes: crate::paths::asset_cache_max_bytes(),
                ..AssetCacheLimits::default()
            },
        ) {
            Ok(store) => return store,
            Err(error) => warn!("settings disk cache unavailable ({error}); in-memory only"),
        }
    }
    // The disk-less store cannot fail to open; the loop extracts it without an
    // `unwrap`/`expect` and runs exactly once.
    loop {
        match AssetStore::new(
            Arc::clone(&fetcher),
            None,
            AssetCacheLimits {
                max_bytes: crate::paths::asset_cache_max_bytes(),
                ..AssetCacheLimits::default()
            },
        ) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory settings store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk settings-asset cache directory
/// (`<cache>/sl-client-bevy-viewer/envcache`), from `XDG_CACHE_HOME` or
/// `~/.cache`, or `None` when neither is set (the store then runs in-memory only).
fn settings_cache_dir() -> Option<PathBuf> {
    crate::paths::asset_cache_dir("envcache")
}

/// Refresh the store fetcher's `ViewerAsset` capability URL each time the region's
/// capability map is (re)discovered, then re-issue any parked resolves.
pub fn update_environment_asset_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    mut manager: ResMut<EnvironmentAssetManager>,
) {
    let mut caps_refreshed = false;
    for SlCapabilities(map) in capabilities.read() {
        manager.set_cap_url(map.get(CAP_VIEWER_ASSET).cloned());
        caps_refreshed = true;
    }
    // A capability refresh (region cross / reconnect) re-arms any settings asset a
    // post-cap transient failure had marked permanently unavailable.
    if caps_refreshed {
        manager.rearm_unavailable();
    }
    manager.retry_pending();
}

/// Poll the in-flight settings fetch+decode tasks, moving finished ones into the
/// decoded map (or marking them unavailable on failure).
pub fn poll_environment_assets(mut manager: ResMut<EnvironmentAssetManager>) {
    // Collect the finished ids first — the borrow of the task map cannot overlap
    // the mutation of the decoded / unavailable maps.
    let mut finished: Vec<(AssetKey, Option<EnvironmentAsset>)> = Vec::new();
    for (&id, task) in &mut manager.inflight {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((id, result));
        }
    }
    for (id, result) in finished {
        let _removed = manager.inflight.remove(&id);
        match result {
            Some(asset) => {
                debug!("environment settings asset {} decoded", id.uuid());
                let _prev = manager.decoded.insert(id, Arc::new(asset));
            }
            None => {
                let _inserted = manager.unavailable.insert(id);
            }
        }
    }
}
