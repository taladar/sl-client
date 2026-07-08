//! The texture store: a weak-reference cache that fetches, decodes, and keeps
//! level-of-detail-aware textures, never fetching or decoding one twice while it
//! is still referenced.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use bytes::Bytes;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry as MapEntry;
use sl_asset_sched::{PriorityGate, StoreStats, run_cpu};
use sl_proto::{DiscardLevel, TextureKey, j2c};

use crate::decode::{DecodeError, DecodedImage, decode_j2c, downsample};
use crate::disk::{CacheLimits, TextureDiskCache};
use crate::entry::{Codestream, TextureEntry};
use crate::fetcher::{FetchError, RemoteTextureSource, TextureFetcher};
use crate::schedule::{Priority, TextureProgress, TextureRequest};

/// Maximum number of texture requests fetching/decoding at once; the rest queue
/// behind the priority gate.
const DEFAULT_INFLIGHT: usize = 16;

/// A failure to obtain a decoded texture at the requested level of detail.
#[derive(Debug, thiserror::Error)]
pub enum TextureError {
    /// The codestream could not be fetched.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// The codestream could not be decoded.
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// A CPU (decode/downsample) task was lost before producing a result.
    #[error("texture worker did not produce a result")]
    WorkerLost,
    /// All requesters withdrew before the work started.
    #[error("texture request cancelled")]
    Cancelled,
}

/// The shared inner state of a [`TextureStore`].
#[derive(Debug)]
struct StoreInner {
    /// Live textures, held only weakly so pointer counts drive collection.
    map: DashMap<TextureKey, Weak<TextureEntry>>,
    /// The frontend-supplied network fetcher.
    fetcher: Arc<dyn TextureFetcher>,
    /// The optional on-disk cache (our own dedicated directory).
    disk: Option<TextureDiskCache>,
    /// Bounds simultaneous CPU decode/downsample work.
    decode_permits: async_lock::Semaphore,
    /// The priority-ordered admission gate bounding in-flight requests.
    gate: PriorityGate,
    /// Monotonic source of unique request ids.
    request_counter: AtomicU64,
    /// Cumulative disk-cache hits (codestreams served from disk).
    cache_hits: AtomicU64,
    /// Cumulative entries garbage-collected by [`sweep`](TextureStore::sweep).
    collected: AtomicU64,
}

/// A cloneable handle to a texture fetch/decode/cache store.
///
/// The store hands out `Arc<TextureEntry>`; it keeps only `Weak` references, so a
/// texture is collectible once the last external `Arc` drops. Requests for a
/// texture already in flight or in memory share its work — a texture is never
/// fetched or decoded twice while referenced.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `TextureStore` reads clearly"
)]
#[derive(Clone, Debug)]
pub struct TextureStore(Arc<StoreInner>);

impl TextureStore {
    /// Builds a store over `fetcher`, optionally backed by an on-disk cache at
    /// `disk_dir`. Decode concurrency is bounded to the CPU count.
    ///
    /// # Errors
    ///
    /// Returns an error if `disk_dir` is given but its cache cannot be opened.
    pub fn new(
        fetcher: Arc<dyn TextureFetcher>,
        disk_dir: Option<std::path::PathBuf>,
        limits: CacheLimits,
    ) -> std::io::Result<Self> {
        let disk = match disk_dir {
            Some(dir) => Some(TextureDiskCache::open(dir, limits)?),
            None => None,
        };
        Ok(Self(Arc::new(StoreInner {
            map: DashMap::new(),
            fetcher,
            disk,
            decode_permits: async_lock::Semaphore::new(num_cpus::get().max(1)),
            gate: PriorityGate::new(DEFAULT_INFLIGHT),
            request_counter: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            collected: AtomicU64::new(0),
        })))
    }

    /// Returns the live entry for `id` without fetching anything, or `None` if it
    /// is not currently in memory.
    #[must_use]
    pub fn peek(&self, id: TextureKey) -> Option<Arc<TextureEntry>> {
        self.0
            .map
            .get(&id)
            .map(|reference| reference.clone())
            .and_then(|weak| weak.upgrade())
    }

    /// Drops dead weak references from the map. Cheap; call periodically.
    pub fn sweep(&self) {
        let before = self.0.map.len();
        self.0.map.retain(|_id, weak| weak.strong_count() > 0);
        let removed = before.saturating_sub(self.0.map.len());
        self.0
            .collected
            .fetch_add(as_u64(removed), Ordering::Relaxed);
    }

    /// A point-in-time snapshot of the store's fetch/decode pipeline: its live
    /// entries bucketed by progress, the in-memory byte footprint, and the
    /// cumulative cache-hit and garbage-collected counters.
    #[must_use]
    pub fn stats(&self) -> StoreStats {
        let mut stats = StoreStats::default();
        for reference in &self.0.map {
            let Some(entry) = reference.value().upgrade() else {
                continue;
            };
            stats.in_memory = stats.in_memory.saturating_add(1);
            let bucket = match entry.progress() {
                TextureProgress::Queued => &mut stats.queued,
                TextureProgress::ReadingDisk { .. } => &mut stats.reading_disk,
                TextureProgress::Downloading { .. } => &mut stats.downloading,
                TextureProgress::Decoding => &mut stats.decoding,
                TextureProgress::Ready(_level) => &mut stats.ready,
                TextureProgress::Failed => &mut stats.failed,
                TextureProgress::Cancelled => &mut stats.cancelled,
            };
            *bucket = bucket.saturating_add(1);
            let codestream = as_u64(entry.codestream.load().covered());
            let pixels = entry.image().map_or(0, |image| as_u64(image.pixels.len()));
            stats.bytes = stats
                .bytes
                .saturating_add(codestream)
                .saturating_add(pixels);
        }
        stats.cache_hits = self.0.cache_hits.load(Ordering::Relaxed);
        stats.collected = self.0.collected.load(Ordering::Relaxed);
        stats
    }

    /// A snapshot of the store's admission gate (capacity / in-flight / waiting).
    #[must_use]
    pub fn gate_stats(&self) -> sl_asset_sched::GateStats {
        self.0.gate.stats()
    }

    /// Registers a non-blocking, observable, cancellable, re-prioritizable
    /// request for `id` at least `target` resolution with the given `priority`.
    /// Drive it to completion with [`TextureRequest::resolved`].
    #[must_use]
    pub fn request(
        &self,
        id: TextureKey,
        target: DiscardLevel,
        priority: Priority,
        source: RemoteTextureSource,
    ) -> TextureRequest {
        let entry = self.resolve_entry(id);
        entry.source.store(Arc::new(source));
        let request_id = self.0.request_counter.fetch_add(1, Ordering::Relaxed);
        TextureRequest::new(self.clone(), entry, request_id, priority, target)
    }

    /// The admission gate (used by [`TextureRequest`]).
    pub(crate) fn gate(&self) -> &PriorityGate {
        &self.0.gate
    }

    /// Drives an upgrade with progress reporting: publishes a terminal
    /// [`TextureProgress::Ready`] or [`TextureProgress::Failed`].
    ///
    /// The effective target is coalesced to the *finest* level any live
    /// requester wants (this request's own `target` and the entry's cached
    /// [`target_want`](TextureEntry::target_want)), so two concurrent requests
    /// for the same texture at different LODs trigger a single decode at the
    /// finest-wanted level rather than one decode each. A coarse consumer
    /// harmlessly receives the finer image (downgradeable under memory pressure).
    pub(crate) async fn drive(
        &self,
        entry: &Arc<TextureEntry>,
        target: DiscardLevel,
    ) -> Result<(), TextureError> {
        let target = finest(target, entry.finest_want());
        let outcome = self.upgrade(entry, target).await;
        publish(entry, target, outcome)
    }

    /// Ensures `id` is decoded to at least `target` resolution and returns its
    /// shared entry, fetching and decoding only what is missing.
    ///
    /// # Errors
    ///
    /// Returns [`TextureError`] if the codestream cannot be fetched or decoded.
    pub async fn get(
        &self,
        id: TextureKey,
        target: DiscardLevel,
        source: RemoteTextureSource,
    ) -> Result<Arc<TextureEntry>, TextureError> {
        let entry = self.resolve_entry(id);
        entry.source.store(Arc::new(source));
        if let Some(image) = entry.image()
            && image.discard_level.is_at_least_as_fine_as(target)
        {
            entry.set_progress(TextureProgress::Ready(image.discard_level));
            return Ok(entry);
        }
        let outcome = self.upgrade(&entry, target).await;
        publish(&entry, target, outcome)?;
        Ok(entry)
    }

    /// Changes `entry`'s in-memory level of detail: upgrades (fetch + decode) to
    /// a finer `target`, or downgrades (downsample in place, no decode) to a
    /// coarser one. A coarser downgrade waits for any pixel lease to release
    /// before it frees the finer buffer.
    ///
    /// # Errors
    ///
    /// Returns [`TextureError`] if an upgrade must fetch/decode and that fails.
    pub async fn set_lod(
        &self,
        entry: &Arc<TextureEntry>,
        target: DiscardLevel,
    ) -> Result<(), TextureError> {
        match entry.current_discard() {
            Some(current) if target.get() > current.get() => {
                self.downgrade(entry, target).await;
                if let Some(level) = entry.current_discard() {
                    entry.set_progress(TextureProgress::Ready(level));
                }
                Ok(())
            }
            _other => {
                let outcome = self.upgrade(entry, target).await;
                publish(entry, target, outcome)
            }
        }
    }

    /// Resolves (or creates) the shared entry for `id`, reusing a live one and
    /// replacing a collected (dead) weak reference. Race-free against concurrent
    /// callers via the map's per-key entry API.
    fn resolve_entry(&self, id: TextureKey) -> Arc<TextureEntry> {
        if let Some(weak) = self.0.map.get(&id).map(|reference| reference.clone())
            && let Some(strong) = weak.upgrade()
        {
            return strong;
        }
        match self.0.map.entry(id) {
            MapEntry::Occupied(mut occupied) => match occupied.get().upgrade() {
                Some(strong) => strong,
                None => {
                    let entry = TextureEntry::new(id);
                    let _old = occupied.insert(Arc::downgrade(&entry));
                    entry
                }
            },
            MapEntry::Vacant(vacant) => {
                let entry = TextureEntry::new(id);
                let _inserted = vacant.insert(Arc::downgrade(&entry));
                entry
            }
        }
    }

    /// The single-flight upgrade path: under the entry's write lock, ensure the
    /// codestream covers `target`, decode it, and publish the finer image.
    async fn upgrade(
        &self,
        entry: &Arc<TextureEntry>,
        target: DiscardLevel,
    ) -> Result<(), TextureError> {
        let guard = entry.write_lock.lock().await;
        if let Some(image) = entry.image()
            && image.discard_level.is_at_least_as_fine_as(target)
        {
            drop(guard);
            return Ok(());
        }
        self.ensure_codestream(entry, target, false).await?;
        let bytes = entry.codestream.load().bytes.clone();
        entry.set_progress(TextureProgress::Decoding);
        let decoded = match self.decode(bytes, target).await {
            Ok(decoded) => decoded,
            Err(error) => {
                // The per-LOD byte estimate is only an *estimate* of the prefix a
                // level needs; for a texture that compresses worse than the
                // viewer's assumed 1/8 rate it can fall short and hand OpenJPEG a
                // codestream truncated mid-tile-part, which it rejects. If we do
                // not yet have the whole asset, grow to the full-resolution bound
                // (the uncompressed size — always enough) and decode once more.
                // Only a texture that actually failed pays this cost, so the common
                // case keeps the fast, small estimate fetch.
                if entry.codestream.load().complete {
                    return Err(error);
                }
                self.ensure_codestream(entry, target, true).await?;
                let bytes = entry.codestream.load().bytes.clone();
                self.decode(bytes, target).await?
            }
        };
        entry.image.store(Some(Arc::new(decoded)));
        drop(guard);
        Ok(())
    }

    /// The downgrade path: downsample the current pixels to `target` (no decode),
    /// then swap them in once no lease is holding the finer buffer.
    async fn downgrade(&self, entry: &Arc<TextureEntry>, target: DiscardLevel) {
        let guard = entry.write_lock.lock().await;
        let Some(image) = entry.image() else {
            drop(guard);
            return;
        };
        if target.get() <= image.discard_level.get() {
            drop(guard);
            return;
        }
        let Some(coarser) = self.run_cpu(move || downsample(&image, target)).await else {
            drop(guard);
            return;
        };
        let usage = entry.usage.write().await;
        entry.image.store(Some(Arc::new(coarser)));
        drop(usage);
        drop(guard);
    }

    /// Grows `entry`'s codestream until it covers `target` (or the whole asset),
    /// pulling from the disk cache first and the network for the rest. Assumes
    /// the caller holds the entry's write lock.
    ///
    /// `full` selects how many bytes "enough" is. Normally (`false`) it is the
    /// viewer's per-LOD byte *estimate* (`data_size` / `calcDataSizeJ2C`), a small
    /// prefix that decodes to `target` for the common, well-compressing texture —
    /// the fast path. When a decode of that prefix fails (a texture that
    /// compresses worse than the assumed 1/8 rate, whose codestream the estimate
    /// truncates mid-tile-part), the caller retries with `full = true`, which
    /// grows to the uncompressed-size upper bound — always enough to cover the
    /// whole codestream — so only the rare failing texture pays for the larger
    /// fetch.
    async fn ensure_codestream(
        &self,
        entry: &Arc<TextureEntry>,
        target: DiscardLevel,
        full: bool,
    ) -> Result<(), TextureError> {
        loop {
            let current = entry.codestream.load_full();
            if current.complete {
                return Ok(());
            }
            // No header yet ⇒ probe with `FIRST_PACKET_SIZE` first to read it.
            let need = entry.header().map_or(j2c::FIRST_PACKET_SIZE, |header| {
                if full {
                    header.full_data_size_bound()
                } else {
                    target.data_size(&header)
                }
            });
            let covered = current.covered();
            if covered >= need {
                return Ok(());
            }
            let grew = self.fetch_more(entry, &current, need).await?;
            if !grew {
                // No progress possible (server returned nothing new); decode with
                // whatever prefix is in hand.
                return Ok(());
            }
        }
    }

    /// Fetches more codestream bytes for `entry` toward `need` — from the disk
    /// cache when nothing is loaded yet, otherwise the network — appends them,
    /// and persists the grown prefix to disk. Returns whether any bytes were
    /// added.
    async fn fetch_more(
        &self,
        entry: &Arc<TextureEntry>,
        current: &Codestream,
        need: usize,
    ) -> Result<bool, TextureError> {
        let covered = current.covered();
        if covered == 0
            && let Some(bytes) = self
                .0
                .disk
                .as_ref()
                .and_then(|disk| disk.read(entry.id.uuid()))
            && !bytes.is_empty()
        {
            entry.set_progress(TextureProgress::ReadingDisk {
                read: bytes.len(),
                total: need,
            });
            self.0.cache_hits.fetch_add(1, Ordering::Relaxed);
            store_codestream(entry, bytes, false);
            return Ok(true);
        }
        entry.set_progress(TextureProgress::Downloading {
            covered,
            needed: need,
        });
        // The texture's source (default CDN, or a bake's appearance-service URL)
        // was recorded on the entry by `get`/`request`; the fetcher picks the
        // endpoint from it.
        let source = entry.source.load_full();
        let chunk = self
            .0
            .fetcher
            .fetch_range(entry.id, &source, covered, need)
            .await?;
        if chunk.whole {
            self.persist(entry.id, &chunk.bytes);
            let empty = chunk.bytes.is_empty();
            store_codestream(entry, chunk.bytes, true);
            return Ok(!empty);
        }
        if chunk.bytes.is_empty() {
            return Ok(false);
        }
        let mut grown = Vec::with_capacity(covered.saturating_add(chunk.bytes.len()));
        grown.extend_from_slice(&current.bytes);
        grown.extend_from_slice(&chunk.bytes);
        let grown = Bytes::from(grown);
        self.persist(entry.id, &grown);
        store_codestream(entry, grown, false);
        Ok(true)
    }

    /// Writes a codestream to the on-disk cache, if one is configured. Best
    /// effort: a write failure is logged and ignored (the cache is a hint).
    fn persist(&self, id: TextureKey, codestream: &[u8]) {
        if let Some(disk) = self.0.disk.as_ref()
            && let Err(error) = disk.write(id.uuid(), codestream, now_unix())
        {
            tracing::warn!(%id, %error, "texture disk-cache write failed");
        }
    }

    /// Decodes `bytes` to RGBA8 at `target` on the CPU pool, permit-bounded.
    async fn decode(
        &self,
        bytes: Bytes,
        target: DiscardLevel,
    ) -> Result<DecodedImage, TextureError> {
        match self.run_cpu(move || decode_j2c(&bytes, target)).await {
            Some(result) => Ok(result?),
            None => Err(TextureError::WorkerLost),
        }
    }

    /// Runs a CPU-bound task on the shared rayon bridge, bounded by the decode
    /// semaphore. Returns `None` if the worker was lost (e.g. a panic).
    async fn run_cpu<T, F>(&self, task: F) -> Option<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        run_cpu(&self.0.decode_permits, task).await
    }
}

/// Publishes the terminal progress for an upgrade `outcome`: [`Ready`] at the
/// level actually in hand (falling back to `target`) on success, [`Failed`] on
/// error. Shared by [`drive`](TextureStore::drive), [`get`](TextureStore::get),
/// and [`set_lod`](TextureStore::set_lod) so every completion path leaves the
/// entry's observable progress truthful (not stuck at `Decoding`).
///
/// [`Ready`]: TextureProgress::Ready
/// [`Failed`]: TextureProgress::Failed
fn publish(
    entry: &Arc<TextureEntry>,
    target: DiscardLevel,
    outcome: Result<(), TextureError>,
) -> Result<(), TextureError> {
    match outcome {
        Ok(()) => {
            let level = entry.current_discard().unwrap_or(target);
            entry.set_progress(TextureProgress::Ready(level));
            Ok(())
        }
        Err(error) => {
            entry.set_progress(TextureProgress::Failed);
            Err(error)
        }
    }
}

/// The finer (smaller discard) of an explicit target and an optional cached
/// finest-wanted level. Used to coalesce concurrent mixed-LOD requests to a
/// single decode at the finest level any live requester wants.
const fn finest(target: DiscardLevel, want: Option<DiscardLevel>) -> DiscardLevel {
    match want {
        Some(want) if want.is_at_least_as_fine_as(target) => want,
        _other => target,
    }
}

/// Stores a new codestream prefix on `entry` and refreshes its cached header.
fn store_codestream(entry: &Arc<TextureEntry>, bytes: Bytes, complete: bool) {
    entry
        .codestream
        .store(Arc::new(Codestream { bytes, complete }));
    let _header = entry.header();
}

/// A `usize` widened to `u64` for a stats counter, saturating on the (only
/// theoretically possible, on a >64-bit target) overflow.
fn as_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// The current time in whole unix seconds, for LRU stamping (0 on error).
fn now_unix() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u32::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{TextureStore, now_unix};
    use crate::fetcher::{FetchChunk, FetchError, RemoteTextureSource, TextureFetcher};
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_proto::{DiscardLevel, TextureKey};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fetcher that returns a fixed byte blob and counts its calls, for
    /// exercising the store's fetch/cache/single-flight paths without a network.
    #[derive(Debug)]
    struct CountingFetcher {
        /// The bytes returned for any range (as a whole-asset `200` response).
        blob: Bytes,
        /// Shared count of how many times `fetch_range` was called.
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl TextureFetcher for CountingFetcher {
        async fn fetch_range(
            &self,
            _id: TextureKey,
            _source: &RemoteTextureSource,
            _start: usize,
            _end: usize,
        ) -> Result<FetchChunk, FetchError> {
            let _previous = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(FetchChunk {
                bytes: self.blob.clone(),
                whole: true,
            })
        }
    }

    /// A store over a counting fetcher (no disk cache), plus the shared call
    /// counter so a test can assert how many fetches happened.
    fn store_with(blob: Bytes) -> (TextureStore, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher: Arc<dyn TextureFetcher> = Arc::new(CountingFetcher {
            blob,
            calls: Arc::clone(&calls),
        });
        let store = TextureStore::new(fetcher, None, crate::disk::CacheLimits::default())
            .unwrap_or_else(|_error| unreachable!("no disk cache cannot fail"));
        (store, calls)
    }

    #[test]
    fn peek_and_sweep_track_weak_references() {
        let (store, _calls) = store_with(Bytes::from_static(b"not-a-texture"));
        let id = TextureKey::from(sl_proto::Uuid::from_u128(1));
        assert!(store.peek(id).is_none());
        // Resolve a live entry and hold it: peek finds the same object.
        let entry = store.resolve_entry(id);
        assert!(store.peek(id).is_some());
        // Drop the last strong ref; sweep collects the now-dead weak.
        drop(entry);
        store.sweep();
        assert!(store.peek(id).is_none());
    }

    #[test]
    fn disk_cache_avoids_a_second_network_fetch() {
        // 700 zero bytes cover FIRST_PACKET_SIZE but are not a valid J2C image, so
        // decode fails and the entry is dropped. With a disk cache, the codestream
        // was persisted, so a later `get` reads disk and never re-fetches.
        let dir = std::env::temp_dir().join(format!("sl-texture-store-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher: Arc<dyn TextureFetcher> = Arc::new(CountingFetcher {
            blob: Bytes::from(vec![0_u8; 700]),
            calls: Arc::clone(&calls),
        });
        let store = TextureStore::new(
            fetcher,
            Some(dir.clone()),
            crate::disk::CacheLimits::default(),
        )
        .unwrap_or_else(|_error| unreachable!("disk cache open"));
        let id = TextureKey::from(sl_proto::Uuid::from_u128(2));
        pollster::block_on(async {
            let _first = store
                .get(id, DiscardLevel::FULL, RemoteTextureSource::Default)
                .await;
            let _second = store
                .get(id, DiscardLevel::FULL, RemoteTextureSource::Default)
                .await;
        });
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second get served from disk"
        );
        let _removed = fs_err::remove_dir_all(&dir);
    }

    #[test]
    fn now_unix_is_nonzero() {
        assert!(now_unix() > 0);
    }

    #[test]
    fn request_reports_progress_and_shares_the_entry() {
        use crate::schedule::{Priority, TextureProgress};
        let (store, _calls) = store_with(Bytes::from(vec![0_u8; 700]));
        let id = TextureKey::from(sl_proto::Uuid::from_u128(3));
        let request = store.request(
            id,
            DiscardLevel::FULL,
            Priority::new(5),
            RemoteTextureSource::Default,
        );
        // A fresh request starts queued and exposes the shared entry.
        assert_eq!(request.progress(), TextureProgress::Queued);
        let entry = request.entry();
        assert_eq!(entry.id(), id);
        // Driving it fails to decode zeros, ending in the Failed progress state.
        pollster::block_on(async {
            let _outcome = request.resolved().await;
        });
        assert_eq!(request.progress(), TextureProgress::Failed);
    }

    #[test]
    fn mixed_lod_requests_coalesce_to_the_finest() {
        use super::finest;
        use crate::schedule::Priority;
        // Two concurrent requesters for one texture at different LODs: one coarse
        // (discard 3), one fine (full resolution). The store must decode once at
        // the finest-wanted level so both are satisfied by the single image
        // (rather than one decode per LOD, which would depend on arrival order).
        let (store, _calls) = store_with(Bytes::from(vec![0_u8; 700]));
        let id = TextureKey::from(sl_proto::Uuid::from_u128(5));
        let coarse = DiscardLevel::from_clamped(3);
        let coarse_request =
            store.request(id, coarse, Priority::new(1), RemoteTextureSource::Default);
        let fine_request = store.request(
            id,
            DiscardLevel::FULL,
            Priority::new(1),
            RemoteTextureSource::Default,
        );
        let entry = fine_request.entry();
        // The cached finest-wanted level reflects the finer of the two requests.
        assert_eq!(entry.finest_want(), Some(DiscardLevel::FULL));
        // So `drive` for the *coarse* request still targets full resolution: the
        // single decode at the finest level satisfies the coarse consumer too.
        assert_eq!(finest(coarse, entry.finest_want()), DiscardLevel::FULL);
        // A lone coarse target with no finer requester is left untouched.
        assert_eq!(finest(coarse, None), coarse);
        drop(coarse_request);
        drop(fine_request);
    }

    #[test]
    fn get_publishes_failed_progress_on_decode_error() {
        use crate::schedule::{Priority, TextureProgress};
        // 700 zero bytes are not a valid J2C image, so the decode fails: `get`
        // must publish the terminal Failed, not leave progress stuck at Decoding.
        let (store, _calls) = store_with(Bytes::from(vec![0_u8; 700]));
        let id = TextureKey::from(sl_proto::Uuid::from_u128(7));
        // A held request keeps the shared entry alive so its progress is readable
        // after the failing `get` (which otherwise drops its only strong ref).
        let held = store.request(
            id,
            DiscardLevel::FULL,
            Priority::new(1),
            RemoteTextureSource::Default,
        );
        pollster::block_on(async {
            let _outcome = store
                .get(id, DiscardLevel::FULL, RemoteTextureSource::Default)
                .await;
        });
        assert_eq!(held.progress(), TextureProgress::Failed);
    }

    #[test]
    fn stats_bucket_live_entries_and_count_collection() {
        use crate::schedule::Priority;
        let (store, _calls) = store_with(Bytes::from(vec![0_u8; 700]));
        let id = TextureKey::from(sl_proto::Uuid::from_u128(6));
        let request = store.request(
            id,
            DiscardLevel::FULL,
            Priority::new(1),
            RemoteTextureSource::Default,
        );
        // One queued entry, held in memory, nothing collected yet.
        let queued = store.stats();
        assert_eq!(queued.in_memory, 1);
        assert_eq!(queued.queued, 1);
        assert_eq!(queued.collected, 0);
        // Driving fails to decode zeros, so the entry lands in the Failed bucket.
        pollster::block_on(async {
            let _outcome = request.resolved().await;
        });
        let failed = store.stats();
        assert_eq!(failed.failed, 1);
        assert_eq!(failed.ready, 0);
        // Dropping the last requester and sweeping collects it and counts one GC.
        drop(request);
        store.sweep();
        let swept = store.stats();
        assert_eq!(swept.in_memory, 0);
        assert_eq!(swept.collected, 1);
    }

    #[test]
    fn dropping_the_last_request_cancels_and_collects() {
        use crate::schedule::Priority;
        let (store, _calls) = store_with(Bytes::from_static(b"x"));
        let id = TextureKey::from(sl_proto::Uuid::from_u128(4));
        let first = store.request(
            id,
            DiscardLevel::FULL,
            Priority::new(1),
            RemoteTextureSource::Default,
        );
        let second = store.request(
            id,
            DiscardLevel::FULL,
            Priority::new(9),
            RemoteTextureSource::Default,
        );
        // Two requesters share one entry; effective = max(1, 9) + boost(2)=4.
        let entry = first.entry();
        assert_eq!(entry.interest(), 2);
        assert_eq!(entry.effective_priority(), Priority::new(13));
        // Dropping one requester leaves the other's interest and lowers to max(1).
        drop(second);
        assert_eq!(entry.interest(), 1);
        assert_eq!(entry.effective_priority(), Priority::new(1));
        // Dropping the last and the local entry ref makes it collectible.
        drop(first);
        drop(entry);
        store.sweep();
        assert!(store.peek(id).is_none());
    }
}
