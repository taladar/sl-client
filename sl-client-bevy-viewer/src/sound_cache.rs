//! Shared fetch / decode / cache of short SL sound assets, feeding the in-world
//! spatial sounds (`viewer-in-world-sounds`) and the viewer's own UI sounds
//! (`viewer-ui-sound-effects`).
//!
//! A sound is an `AssetType::Sound` UUID (Ogg Vorbis on the wire). This resource
//! fetches its bytes over the region's `ViewerAsset` capability, decodes them
//! **once** off the render thread into a [`DecodedClip`] (resampled to the
//! mixer's device rate so the sampler never resamples per play), and caches the
//! result by asset id. Every producer asks for a clip by id and plays it through
//! the one [`sl_audio::Mixer`]; nothing decodes a sound twice or opens its own
//! device.
//!
//! It mirrors [`AnimationManager`](crate::animations::AnimationManager): an
//! [`AssetStore`] over `ViewerAsset` with an on-disk byte cache, in-flight
//! resolve tasks on the [`IoTaskPool`], and a *pending* set for ids requested
//! before the fetch could run — here that means before the `ViewerAsset`
//! capability **and** the mixer's device sample rate are both known, since the
//! decode needs the target rate. Both are supplied each frame and the pending
//! set is drained once they are.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_audio::{DecodedClip, Mixer, decode_clip};
use sl_client_bevy::{
    AssetCacheLimits, AssetKey, AssetStore, AssetType, BevyAssetFetcher, BlobFetcher,
    CAP_VIEWER_ASSET, GateStats, SlCapabilities, StoreStats,
};

/// The fetch / decode / cache pipeline for short SL sound assets, shared by every
/// sound producer.
#[derive(Resource)]
pub(crate) struct SoundCache {
    /// The generic-asset store doing the `ViewerAsset` fetch, dedupe, off-thread
    /// work and on-disk caching of the encoded sound bytes.
    store: AssetStore,
    /// The store's HTTP fetcher, kept so its `ViewerAsset` capability URL can be
    /// refreshed on a region change.
    fetcher: Arc<BevyAssetFetcher>,
    /// The background fetch+decode task per sound id, polled to completion by
    /// [`poll_sound_cache`]; presence means "already being resolved".
    inflight: HashMap<AssetKey, Task<Option<DecodedClip>>>,
    /// Successfully decoded clips by id, shared across every voice that plays the
    /// sound so it is fetched and decoded once.
    clips: HashMap<AssetKey, DecodedClip>,
    /// Ids whose fetch or decode failed, so [`request`](Self::request) does not
    /// retry them forever.
    unavailable: HashSet<AssetKey>,
    /// Ids requested before the `ViewerAsset` capability or the device sample rate
    /// was known, held (not marked unavailable) until both arrive. Drained by
    /// [`retry_pending`](Self::retry_pending).
    pending: HashSet<AssetKey>,
    /// The mixer's device sample rate, the decode target so the sampler never
    /// resamples per play. `None` until the audio device has started.
    sample_rate: Option<NonZeroU32>,
}

impl SoundCache {
    /// Build the cache over a fresh [`BevyAssetFetcher`], backed by the on-disk
    /// asset cache when a cache directory is available (falling back to an
    /// in-memory-only store).
    pub(crate) fn new() -> Self {
        let fetcher = Arc::new(BevyAssetFetcher::new());
        let store = build_asset_store(&fetcher, sound_cache_dir());
        Self {
            store,
            fetcher,
            inflight: HashMap::new(),
            clips: HashMap::new(),
            unavailable: HashSet::new(),
            pending: HashSet::new(),
            sample_rate: None,
        }
    }

    /// Ensure `id` is being resolved: a nil id, an already-decoded id, one in
    /// flight, or one known unavailable is ignored. If the `ViewerAsset`
    /// capability or the device sample rate is not known yet the id is parked in
    /// [`pending`](Self::pending) rather than fetched (a fetch without the cap
    /// would fail, and a decode without the rate has no target); it is re-issued
    /// by [`retry_pending`](Self::retry_pending) once both are set. Idempotent.
    pub(crate) fn request(&mut self, id: AssetKey) {
        if id.uuid().is_nil()
            || self.clips.contains_key(&id)
            || self.inflight.contains_key(&id)
            || self.unavailable.contains(&id)
        {
            return;
        }
        let Some(target_rate) = self.sample_rate else {
            let _inserted = self.pending.insert(id);
            return;
        };
        if !self.fetcher.has_cap_url() {
            let _inserted = self.pending.insert(id);
            return;
        }
        self.pending.remove(&id);
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            // The fetch and the decode both run on this IoTaskPool thread, so the
            // render thread never touches sound bytes.
            let bytes = match store.get(id, AssetType::Sound).await {
                Ok(entry) => match entry.data() {
                    Some(data) => data.to_vec(),
                    None => {
                        warn!("sound {} fetched but has no data", id.uuid());
                        return None;
                    }
                },
                Err(error) => {
                    warn!("fetching sound {} over ViewerAsset: {error}", id.uuid());
                    return None;
                }
            };
            match decode_clip(bytes, target_rate) {
                Ok(clip) => Some(clip),
                Err(error) => {
                    warn!("decoding sound {}: {error}", id.uuid());
                    None
                }
            }
        });
        let _prev = self.inflight.insert(id, task);
    }

    /// The decoded clip for `id`, once resolved, or `None` if it is still in
    /// flight, has no fetchable asset, or failed decoding.
    pub(crate) fn clip(&self, id: AssetKey) -> Option<&DecodedClip> {
        self.clips.get(&id)
    }

    /// Whether `id` is known to have no playable clip (its fetch or decode
    /// failed), so a producer waiting on it can give up rather than wait forever.
    pub(crate) fn is_unavailable(&self, id: AssetKey) -> bool {
        self.unavailable.contains(&id)
    }

    /// The mixer's device sample rate (the decode target), once the audio device
    /// has started. Skin-bundled UI sounds decode their own bytes at this rate.
    pub(crate) const fn device_sample_rate(&self) -> Option<NonZeroU32> {
        self.sample_rate
    }

    /// A point-in-time snapshot of the sound-clip fetch/decode pipeline, for the
    /// F3 diagnostics overlay: entry counts bucketed by stage plus the cumulative
    /// disk-cache-hit / GC counters. Delegates to the wrapped [`AssetStore`].
    pub(crate) fn stats(&self) -> StoreStats {
        self.store.stats()
    }

    /// A point-in-time snapshot of the sound store's admission gate: its
    /// concurrency capacity, in-flight slots, and queued waiters.
    pub(crate) fn gate_stats(&self) -> GateStats {
        self.store.gate_stats()
    }

    /// How many resolves are parked outside the store's own accounting — held
    /// until both the `ViewerAsset` capability and the device sample rate are
    /// known (see [`pending`](Self::pending)) — so the pipeline overlay does not
    /// report "nothing left to load" while such work is still outstanding.
    pub(crate) fn deferred_count(&self) -> usize {
        self.pending.len()
    }

    /// Point the store's fetcher at the region's current `ViewerAsset` URL.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
    }

    /// Record the mixer's device sample rate (the decode target). A change (a
    /// device hot-plug to a differently-clocked device) re-arms any parked
    /// requests; already-decoded clips keep their old rate and the sampler
    /// resamples them, which is rare and cheap.
    const fn set_sample_rate(&mut self, rate: Option<NonZeroU32>) {
        self.sample_rate = rate;
    }

    /// Re-issue any resolves parked before the capability / sample rate were
    /// known (see [`pending`](Self::pending)), now that they are. A no-op while
    /// either is missing or nothing is pending.
    fn retry_pending(&mut self) {
        if self.pending.is_empty() || self.sample_rate.is_none() || !self.fetcher.has_cap_url() {
            return;
        }
        let pending: Vec<AssetKey> = self.pending.drain().collect();
        for id in pending {
            self.request(id);
        }
    }

    /// Re-park every sound previously marked [`unavailable`](Self::unavailable) so
    /// the next [`retry_pending`](Self::retry_pending) re-fetches it. Called on a
    /// capability refresh (a region cross / reconnect): a sound whose fetch failed
    /// transiently would otherwise stay silent for the session.
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
/// Mirrors the animation / wearable asset-store builders.
fn build_asset_store(fetcher: &Arc<BevyAssetFetcher>, disk_dir: Option<PathBuf>) -> AssetStore {
    let concrete = Arc::clone(fetcher);
    let fetcher: Arc<dyn BlobFetcher> = concrete;
    if let Some(dir) = disk_dir {
        match AssetStore::new(Arc::clone(&fetcher), Some(dir), AssetCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("sound disk cache unavailable ({error}); in-memory only"),
        }
    }
    // The disk-less store cannot fail to open; the loop extracts it without an
    // `unwrap`/`expect` and runs exactly once.
    loop {
        match AssetStore::new(Arc::clone(&fetcher), None, AssetCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory sound store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk sound-asset cache directory
/// (`<cache>/sl-client-bevy-viewer/soundcache`), or `None` when no cache root is
/// available (the store then runs in-memory only).
fn sound_cache_dir() -> Option<PathBuf> {
    crate::paths::asset_cache_dir("soundcache")
}

/// Refresh the store fetcher's `ViewerAsset` capability URL each time the
/// region's capability map is (re)discovered, and re-issue parked requests.
pub(crate) fn update_sound_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    mut cache: ResMut<SoundCache>,
) {
    let mut caps_refreshed = false;
    for SlCapabilities(map) in capabilities.read() {
        cache.set_cap_url(map.get(CAP_VIEWER_ASSET).cloned());
        caps_refreshed = true;
    }
    // A capability refresh (region cross / reconnect) re-arms any sound a post-cap
    // transient failure had marked permanently unavailable.
    if caps_refreshed {
        cache.rearm_unavailable();
    }
    cache.retry_pending();
}

/// Track the mixer's device sample rate into the cache (the decode target) and
/// drain parked requests once it — and the capability — are known.
pub(crate) fn track_sound_sample_rate(
    mixer: Option<NonSend<Mixer>>,
    mut cache: ResMut<SoundCache>,
) {
    let rate = mixer.and_then(|mixer| mixer.sample_rate());
    if rate != cache.sample_rate {
        cache.set_sample_rate(rate);
        cache.retry_pending();
    }
}

/// Poll the in-flight fetch+decode tasks; move each completed clip into the
/// shared cache (a producer reads it the next frame), or record the id
/// unavailable when the fetch / decode failed.
pub(crate) fn poll_sound_cache(mut cache: ResMut<SoundCache>) {
    // Collect the finished ids first — the borrow of the task map cannot overlap
    // the mutation of the clips / unavailable maps.
    let mut finished: Vec<(AssetKey, Option<DecodedClip>)> = Vec::new();
    for (&id, task) in &mut cache.inflight {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((id, result));
        }
    }
    for (id, result) in finished {
        let _removed = cache.inflight.remove(&id);
        match result {
            Some(clip) => {
                debug!(
                    "sound {} decoded ({:.2}s, {} ch)",
                    id.uuid(),
                    clip.duration_seconds(),
                    clip.channels().get()
                );
                let _prev = cache.clips.insert(id, clip);
            }
            None => {
                let _inserted = cache.unavailable.insert(id);
            }
        }
    }
}

/// The [`SoundCache`] plugin: insert the resource and wire the cap / sample-rate
/// tracking and the fetch-task poll. Producers (`world_sounds`, `ui_sounds`) add
/// their own systems that read [`SoundCache`] and the [`Mixer`].
pub(crate) struct SoundCachePlugin;

impl Plugin for SoundCachePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SoundCache::new()).add_systems(
            Update,
            (update_sound_caps, track_sound_sample_rate, poll_sound_cache),
        );
    }
}
