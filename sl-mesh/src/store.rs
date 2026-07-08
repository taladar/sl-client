//! The mesh store: a weak-reference cache that fetches, decodes, and keeps
//! level-of-detail-aware meshes, never fetching or decoding one twice while it
//! is still referenced.
//!
//! Unlike a texture (a progressive codestream), each mesh level of detail is an
//! *independent* block: serving a level fetches that block's byte range,
//! zlib-inflates it, and decodes it — there is no progressive reuse and no
//! in-place downsample. One decode at the finest-wanted level satisfies every
//! concurrent requester.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use bytes::Bytes;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry as MapEntry;
use sl_asset_sched::{PriorityGate, StoreStats, run_cpu};
use sl_proto::{MeshKey, MeshLod};

use crate::decode::{self, MeshDecodeError, MeshHeader, MeshPhysics, parse_header};
use crate::disk::{
    AssetBytes, CacheLimits, FLAG_PHYSICS_CONVEX, FLAG_PHYSICS_MESH, FLAG_SKIN, MeshDiskCache,
    flag_lod, now_unix,
};
use crate::entry::MeshEntry;
use crate::fetcher::{FetchError, MeshFetcher};
use crate::progress::{MeshProgress, MeshRequest, Priority};

/// Maximum number of mesh requests fetching/decoding at once; the rest queue
/// behind the priority gate.
const DEFAULT_INFLIGHT: usize = 16;

/// The largest header prefix the store will probe before giving up parsing a
/// header (a guard against an unbounded fetch of a non-mesh asset).
const MAX_HEADER_PROBE: usize = 64 * 1024;

/// A failure to obtain a decoded mesh at the requested level of detail.
#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    /// The asset could not be fetched.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// A block could not be decoded.
    #[error(transparent)]
    Decode(#[from] MeshDecodeError),
    /// The asset's header was not a recognisable mesh header.
    #[error("asset is not a recognisable mesh")]
    NotAMesh,
    /// The header marks the mesh unavailable (a `404`).
    #[error("mesh is unavailable (404)")]
    NotFound,
    /// The mesh carries no geometry block.
    #[error("mesh carries no geometry")]
    NoGeometry,
    /// A CPU (decode) task was lost before producing a result.
    #[error("mesh worker did not produce a result")]
    WorkerLost,
    /// All requesters withdrew before the work started.
    #[error("mesh request cancelled")]
    Cancelled,
}

/// The shared inner state of a [`MeshStore`].
#[derive(Debug)]
struct StoreInner {
    /// Live meshes, held only weakly so pointer counts drive collection.
    map: DashMap<MeshKey, Weak<MeshEntry>>,
    /// The frontend-supplied network fetcher.
    fetcher: Arc<dyn MeshFetcher>,
    /// The optional on-disk cache (our own dedicated directory).
    disk: Option<MeshDiskCache>,
    /// Bounds simultaneous CPU decode work.
    decode_permits: async_lock::Semaphore,
    /// The priority-ordered admission gate bounding in-flight requests.
    gate: PriorityGate,
    /// Monotonic source of unique request ids.
    request_counter: AtomicU64,
    /// Cumulative disk-cache hits (assets served from disk).
    cache_hits: AtomicU64,
    /// Cumulative entries garbage-collected by [`sweep`](MeshStore::sweep).
    collected: AtomicU64,
}

/// A cloneable handle to a mesh fetch/decode/cache store.
///
/// The store hands out `Arc<MeshEntry>`; it keeps only `Weak` references, so a
/// mesh is collectible once the last external `Arc` drops. Requests for a mesh
/// already in flight or in memory share its work — a mesh is never fetched or
/// decoded twice while referenced.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `MeshStore` reads clearly"
)]
#[derive(Clone, Debug)]
pub struct MeshStore(Arc<StoreInner>);

impl MeshStore {
    /// Builds a store over `fetcher`, optionally backed by an on-disk cache at
    /// `disk_dir`. Decode concurrency is bounded to the CPU count.
    ///
    /// # Errors
    ///
    /// Returns an error if `disk_dir` is given but its cache cannot be opened.
    pub fn new(
        fetcher: Arc<dyn MeshFetcher>,
        disk_dir: Option<std::path::PathBuf>,
        limits: CacheLimits,
    ) -> std::io::Result<Self> {
        let disk = match disk_dir {
            Some(dir) => Some(MeshDiskCache::open(dir, limits)?),
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
    pub fn peek(&self, id: MeshKey) -> Option<Arc<MeshEntry>> {
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
                MeshProgress::Queued => &mut stats.queued,
                MeshProgress::ReadingDisk { .. } => &mut stats.reading_disk,
                MeshProgress::Downloading { .. } => &mut stats.downloading,
                MeshProgress::Decoding => &mut stats.decoding,
                MeshProgress::Ready(_lod) => &mut stats.ready,
                MeshProgress::Failed => &mut stats.failed,
                MeshProgress::Cancelled => &mut stats.cancelled,
            };
            *bucket = bucket.saturating_add(1);
            let asset = entry
                .asset
                .load()
                .as_ref()
                .map_or(0, |asset| as_u64(asset.data.len()));
            stats.bytes = stats.bytes.saturating_add(asset);
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
    /// request for `id` at `target` level with the given `priority`. Drive it to
    /// completion with [`MeshRequest::resolved`].
    #[must_use]
    pub fn request(&self, id: MeshKey, target: MeshLod, priority: Priority) -> MeshRequest {
        let entry = self.resolve_entry(id);
        let request_id = self.0.request_counter.fetch_add(1, Ordering::Relaxed);
        MeshRequest::new(self.clone(), entry, request_id, priority, target)
    }

    /// The admission gate (used by [`MeshRequest`]).
    pub(crate) fn gate(&self) -> &PriorityGate {
        &self.0.gate
    }

    /// Drives a level load with progress reporting, coalescing the target to the
    /// finest level any live requester wants (this request's own `target` and
    /// the entry's cached [`finest_want`](MeshEntry::finest_want)) so one decode
    /// satisfies every concurrent requester. Publishes a terminal
    /// [`MeshProgress::Ready`] or [`MeshProgress::Failed`].
    pub(crate) async fn drive(
        &self,
        entry: &Arc<MeshEntry>,
        target: MeshLod,
    ) -> Result<(), MeshError> {
        let target = finest(target, entry.finest_want());
        let outcome = self.ensure(entry, target, true).await;
        publish(entry, target, outcome)
    }

    /// Ensures `id` is decoded to at least `target` level and returns its shared
    /// entry, fetching and decoding only what is missing.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError`] if the asset cannot be fetched or decoded.
    pub async fn get(&self, id: MeshKey, target: MeshLod) -> Result<Arc<MeshEntry>, MeshError> {
        let entry = self.resolve_entry(id);
        if let Some(current) = entry.current_lod()
            && current.is_at_least_as_fine_as(target)
        {
            entry.set_progress(MeshProgress::Ready(current));
            return Ok(entry);
        }
        let outcome = self.ensure(&entry, target, true).await;
        publish(&entry, target, outcome)?;
        Ok(entry)
    }

    /// Loads `entry`'s geometry at exactly `target`'s level (fetching and
    /// decoding that block, or the nearest available), replacing the current
    /// geometry. There is no in-place downsample — a coarser level is a separate
    /// fetch and decode.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError`] if the fetch or decode fails.
    pub async fn set_lod(&self, entry: &Arc<MeshEntry>, target: MeshLod) -> Result<(), MeshError> {
        let outcome = self.ensure(entry, target, false).await;
        publish(entry, target, outcome)
    }

    /// Ensures `id`'s skin block is decoded and returns its entry. A mesh with no
    /// skin block succeeds with the entry's [`skin`](MeshEntry::skin) left `None`.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError`] if the header or skin block cannot be fetched or
    /// decoded.
    pub async fn get_skin(&self, id: MeshKey) -> Result<Arc<MeshEntry>, MeshError> {
        let entry = self.resolve_entry(id);
        if entry.skin().is_some() {
            return Ok(entry);
        }
        let guard = entry.write_lock.lock().await;
        if entry.skin().is_none() {
            self.ensure_header(&entry).await?;
            let header = entry.header().ok_or(MeshError::NotAMesh)?;
            if let Some(block) = header.skin {
                let (start, end) = block_range(&entry, block);
                let compressed = self.fetch_block(&entry, start, end, FLAG_SKIN).await?;
                let skin = self
                    .run_decode(move || decode::decode_skin(&compressed))
                    .await?;
                entry.skin.store(Some(Arc::new(skin)));
            }
        }
        drop(guard);
        Ok(entry)
    }

    /// Ensures `id`'s physics blocks are decoded and returns its entry. A mesh
    /// with no physics blocks succeeds with [`physics`](MeshEntry::physics) left
    /// `None`.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError`] if a physics block cannot be fetched or decoded.
    pub async fn get_physics(&self, id: MeshKey) -> Result<Arc<MeshEntry>, MeshError> {
        let entry = self.resolve_entry(id);
        if entry.physics().is_some() {
            return Ok(entry);
        }
        let guard = entry.write_lock.lock().await;
        if entry.physics().is_none() {
            self.ensure_header(&entry).await?;
            let header = entry.header().ok_or(MeshError::NotAMesh)?;
            let convex = self.decode_physics_convex(&entry, header).await?;
            let mesh = self.decode_physics_mesh(&entry, header).await?;
            if convex.is_some() || mesh.is_some() {
                entry
                    .physics
                    .store(Some(Arc::new(MeshPhysics { convex, mesh })));
            }
        }
        drop(guard);
        Ok(entry)
    }

    /// Resolves (or creates) the shared entry for `id`, reusing a live one and
    /// replacing a collected (dead) weak reference. Race-free against concurrent
    /// callers via the map's per-key entry API.
    fn resolve_entry(&self, id: MeshKey) -> Arc<MeshEntry> {
        if let Some(weak) = self.0.map.get(&id).map(|reference| reference.clone())
            && let Some(strong) = weak.upgrade()
        {
            return strong;
        }
        match self.0.map.entry(id) {
            MapEntry::Occupied(mut occupied) => match occupied.get().upgrade() {
                Some(strong) => strong,
                None => {
                    let entry = MeshEntry::new(id);
                    let _old = occupied.insert(Arc::downgrade(&entry));
                    entry
                }
            },
            MapEntry::Vacant(vacant) => {
                let entry = MeshEntry::new(id);
                let _inserted = vacant.insert(Arc::downgrade(&entry));
                entry
            }
        }
    }

    /// The single-flight level-load path: under the entry's write lock, pick the
    /// best available block for `target`, fetch it, decode it, and publish the
    /// geometry. When `allow_finer`, a currently loaded finer level satisfies the
    /// request without any work.
    async fn ensure(
        &self,
        entry: &Arc<MeshEntry>,
        target: MeshLod,
        allow_finer: bool,
    ) -> Result<(), MeshError> {
        let guard = entry.write_lock.lock().await;
        if allow_finer
            && let Some(current) = entry.current_lod()
            && current.is_at_least_as_fine_as(target)
        {
            drop(guard);
            return Ok(());
        }
        self.ensure_header(entry).await?;
        let header = entry.header().ok_or(MeshError::NotAMesh)?;
        if header.not_found {
            drop(guard);
            return Err(MeshError::NotFound);
        }
        let lod = header.best_lod(target).ok_or(MeshError::NoGeometry)?;
        if entry.current_lod() == Some(lod) {
            drop(guard);
            return Ok(());
        }
        let block = header.lod(lod).ok_or(MeshError::NoGeometry)?;
        let (start, end) = block_range(entry, block);
        let compressed = self
            .fetch_block(entry, start, end, flag_lod(lod.index()))
            .await?;
        entry.set_progress(MeshProgress::Decoding);
        let decoded = self
            .run_decode(move || decode::decode_lod(&compressed, lod))
            .await?;
        let usage = entry.usage.write().await;
        entry.lod.store(Some(Arc::new(decoded)));
        drop(usage);
        drop(guard);
        Ok(())
    }

    /// Ensures the header bytes are fetched and parsed, populating the entry's
    /// asset region (from the disk cache first, then the network). Assumes the
    /// caller holds the entry's write lock.
    async fn ensure_header(&self, entry: &Arc<MeshEntry>) -> Result<(), MeshError> {
        if entry.asset.load().is_some() {
            return Ok(());
        }
        if let Some(asset) = self.read_disk(entry.id) {
            self.0.cache_hits.fetch_add(1, Ordering::Relaxed);
            entry.set_progress(MeshProgress::ReadingDisk {
                read: asset.data.len(),
                total: asset.data.len(),
            });
            let parsed = parse_header(&asset.data).map(|(header, _size)| header);
            entry.asset.store(Some(Arc::new(asset)));
            let _stored = entry.header.set(parsed);
            if parsed.is_some() {
                return Ok(());
            }
        }
        let (asset, header) = self.fetch_header(entry).await?;
        entry.asset.store(Some(Arc::new(asset)));
        let _stored = entry.header.set(Some(header));
        self.persist(entry);
        Ok(())
    }

    /// Fetches and parses the header, growing the probe until the LLSD map fits
    /// (or the mesh is deemed unrecognisable).
    async fn fetch_header(
        &self,
        entry: &Arc<MeshEntry>,
    ) -> Result<(AssetBytes, MeshHeader), MeshError> {
        let mut want = decode::MESH_HEADER_SIZE;
        loop {
            entry.set_progress(MeshProgress::Downloading {
                covered: 0,
                needed: want,
            });
            let chunk = self.0.fetcher.fetch_range(entry.id, 0, want).await?;
            let data = chunk.bytes;
            if let Some((header, header_size)) = parse_header(&data) {
                let flags = if chunk.whole {
                    all_present_flags(&header, header_size, data.len())
                } else {
                    0
                };
                return Ok((
                    AssetBytes {
                        data,
                        header_size,
                        flags,
                    },
                    header,
                ));
            }
            if chunk.whole || data.len() < want || want >= MAX_HEADER_PROBE {
                return Err(MeshError::NotAMesh);
            }
            want = want.saturating_mul(4);
        }
    }

    /// Fetches the compressed bytes of the block at absolute range `[start, end)`
    /// — from the in-memory asset region when already present, otherwise the
    /// network — updating the asset region and persisting to disk.
    async fn fetch_block(
        &self,
        entry: &Arc<MeshEntry>,
        start: usize,
        end: usize,
        flag: u32,
    ) -> Result<Bytes, MeshError> {
        if let Some(asset) = entry.asset.load_full()
            && asset.flags & flag != 0
            && let Some(bytes) = asset.slice(start, end)
        {
            return Ok(bytes);
        }
        entry.set_progress(MeshProgress::Downloading {
            covered: start,
            needed: end,
        });
        let chunk = self.0.fetcher.fetch_range(entry.id, start, end).await?;
        let current = entry
            .asset
            .load_full()
            .unwrap_or_else(|| Arc::new(AssetBytes::header_only(Bytes::new(), 0)));
        let (block, updated) = if chunk.whole {
            let block = slice_clamped(&chunk.bytes, start, end);
            let flags =
                all_present_flags_from(entry.header(), current.header_size, chunk.bytes.len());
            (
                block,
                AssetBytes {
                    data: chunk.bytes,
                    header_size: current.header_size,
                    flags,
                },
            )
        } else {
            let block = chunk.bytes;
            let updated = current.with_block(start, &block, flag);
            (block, updated)
        };
        entry.asset.store(Some(Arc::new(updated)));
        self.persist(entry);
        Ok(block)
    }

    /// Decodes the convex physics block, if present.
    async fn decode_physics_convex(
        &self,
        entry: &Arc<MeshEntry>,
        header: MeshHeader,
    ) -> Result<Option<crate::decode::PhysicsConvex>, MeshError> {
        let Some(block) = header.physics_convex else {
            return Ok(None);
        };
        let (start, end) = block_range(entry, block);
        let compressed = self
            .fetch_block(entry, start, end, FLAG_PHYSICS_CONVEX)
            .await?;
        let convex = self
            .run_decode(move || decode::decode_physics_convex(&compressed))
            .await?;
        Ok(Some(convex))
    }

    /// Decodes the triangle physics-mesh block, if present.
    async fn decode_physics_mesh(
        &self,
        entry: &Arc<MeshEntry>,
        header: MeshHeader,
    ) -> Result<Option<Vec<crate::decode::Submesh>>, MeshError> {
        let Some(block) = header.physics_mesh else {
            return Ok(None);
        };
        let (start, end) = block_range(entry, block);
        let compressed = self
            .fetch_block(entry, start, end, FLAG_PHYSICS_MESH)
            .await?;
        let mesh = self
            .run_decode(move || decode::decode_physics_mesh(&compressed))
            .await?;
        Ok(Some(mesh))
    }

    /// Reads `id`'s asset from the disk cache, if one is configured.
    fn read_disk(&self, id: MeshKey) -> Option<AssetBytes> {
        self.0.disk.as_ref().and_then(|disk| disk.read(id.uuid()))
    }

    /// Writes the entry's current asset region to the on-disk cache, if one is
    /// configured. Best effort: a write failure is logged and ignored.
    fn persist(&self, entry: &Arc<MeshEntry>) {
        if let Some(disk) = self.0.disk.as_ref()
            && let Some(asset) = entry.asset.load_full()
            && let Err(error) = disk.write(entry.id.uuid(), &asset, now_unix())
        {
            tracing::warn!(id = %entry.id, %error, "mesh disk-cache write failed");
        }
    }

    /// Runs a decode task on the shared rayon bridge, mapping a lost worker to
    /// [`MeshError::WorkerLost`] and a decode failure to [`MeshError::Decode`].
    async fn run_decode<T, F>(&self, task: F) -> Result<T, MeshError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, MeshDecodeError> + Send + 'static,
    {
        match run_cpu(&self.0.decode_permits, task).await {
            Some(result) => Ok(result?),
            None => Err(MeshError::WorkerLost),
        }
    }
}

/// Publishes the terminal progress for a level-load `outcome`: [`Ready`] at the
/// level actually in hand (falling back to `target`) on success, [`Failed`] on
/// error. Shared by [`drive`](MeshStore::drive), [`get`](MeshStore::get), and
/// [`set_lod`](MeshStore::set_lod) so every completion path leaves the entry's
/// observable progress truthful (not stuck at `Downloading` / `Decoding`).
///
/// [`Ready`]: MeshProgress::Ready
/// [`Failed`]: MeshProgress::Failed
fn publish(
    entry: &Arc<MeshEntry>,
    target: MeshLod,
    outcome: Result<(), MeshError>,
) -> Result<(), MeshError> {
    match outcome {
        Ok(()) => {
            let level = entry.current_lod().unwrap_or(target);
            entry.set_progress(MeshProgress::Ready(level));
            Ok(())
        }
        Err(error) => {
            entry.set_progress(MeshProgress::Failed);
            Err(error)
        }
    }
}

/// The finer (higher-detail) of an explicit target and an optional cached
/// finest-wanted level. Used to coalesce concurrent mixed-LOD requests to a
/// single decode at the finest level any live requester wants.
const fn finest(target: MeshLod, want: Option<MeshLod>) -> MeshLod {
    match want {
        Some(want) => target.finer_of(want),
        None => target,
    }
}

/// A `usize` widened to `u64` for a stats counter, saturating on the (only
/// theoretically possible, on a >64-bit target) overflow.
fn as_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// The absolute byte range of `block` given `entry`'s parsed header size.
fn block_range(entry: &Arc<MeshEntry>, block: crate::decode::BlockRef) -> (usize, usize) {
    let header_size = entry
        .asset
        .load()
        .as_ref()
        .map_or(0, |asset| asset.header_size);
    block.range(header_size)
}

/// Clamps `[start, end)` to `bytes` and returns that sub-slice as `Bytes`.
fn slice_clamped(bytes: &Bytes, start: usize, end: usize) -> Bytes {
    let start = start.min(bytes.len());
    let end = end.min(bytes.len()).max(start);
    bytes.slice(start..end)
}

/// The flags bitmask for every block wholly contained in an asset region of
/// `data_len` bytes with the given `header_size`.
fn all_present_flags(header: &MeshHeader, header_size: usize, data_len: usize) -> u32 {
    let mut flags = 0;
    for lod in sl_proto::MeshLod::ALL {
        if let Some(block) = header.lod(lod)
            && covers(&block, header_size, data_len)
        {
            flags |= flag_lod(lod.index());
        }
    }
    if header
        .skin
        .is_some_and(|block| covers(&block, header_size, data_len))
    {
        flags |= FLAG_SKIN;
    }
    if header
        .physics_convex
        .is_some_and(|block| covers(&block, header_size, data_len))
    {
        flags |= FLAG_PHYSICS_CONVEX;
    }
    if header
        .physics_mesh
        .is_some_and(|block| covers(&block, header_size, data_len))
    {
        flags |= FLAG_PHYSICS_MESH;
    }
    flags
}

/// [`all_present_flags`] over an optional header (no flags if it is absent).
fn all_present_flags_from(header: Option<MeshHeader>, header_size: usize, data_len: usize) -> u32 {
    header.map_or(0, |header| {
        all_present_flags(&header, header_size, data_len)
    })
}

/// Whether a block's absolute range fits within `data_len` bytes.
const fn covers(block: &crate::decode::BlockRef, header_size: usize, data_len: usize) -> bool {
    let (_start, end) = block.range(header_size);
    end <= data_len
}

#[cfg(test)]
mod tests {
    use super::{MeshError, MeshStore};
    use crate::disk::CacheLimits;
    use crate::fetcher::{AssetFetcher, FetchChunk, FetchError, MeshFetcher};
    use bytes::Bytes;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use pretty_assertions::assert_eq;
    use sl_proto::{MeshKey, MeshLod, Uuid};
    use sl_wire::Llsd;
    use std::collections::HashMap;
    use std::io::Write as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Builds a minimal one-face-triangle mesh asset (header + one high-LOD
    /// block), returning its bytes.
    fn synth_mesh() -> Result<Vec<u8>, TestError> {
        // One submesh: a triangle spanning the unit position domain.
        let positions = u16_blob(&[0, 0, 0, 0xFFFF, 0, 0, 0, 0xFFFF, 0]);
        let indices = u16_blob(&[0, 1, 2]);
        let submesh = Llsd::Map(HashMap::from([
            ("Position".to_owned(), Llsd::Binary(positions)),
            ("TriangleList".to_owned(), Llsd::Binary(indices)),
            (
                "PositionDomain".to_owned(),
                domain3([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            ),
        ]));
        let block = zlib(&Llsd::Array(vec![submesh]).to_llsd_binary())?;

        // Header naming the high LOD block at offset 0 with the block's size.
        let block_desc = Llsd::Map(HashMap::from([
            ("offset".to_owned(), Llsd::Integer(0)),
            (
                "size".to_owned(),
                Llsd::Integer(i32::try_from(block.len())?),
            ),
        ]));
        let header = Llsd::Map(HashMap::from([
            ("version".to_owned(), Llsd::Integer(1)),
            ("high_lod".to_owned(), block_desc),
        ]));
        let mut asset = header.to_llsd_binary();
        asset.extend_from_slice(&block);
        Ok(asset)
    }

    /// A little-endian `u16` blob.
    fn u16_blob(values: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        for value in values {
            out.push(u8::try_from(value & 0xFF).unwrap_or(0));
            out.push(u8::try_from((value >> 8_u16) & 0xFF).unwrap_or(0));
        }
        out
    }

    /// zlib-compresses `bytes`.
    fn zlib(bytes: &[u8]) -> Result<Vec<u8>, TestError> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes)?;
        Ok(encoder.finish()?)
    }

    /// A `{ Min, Max }` 3-vector domain map.
    fn domain3(min: [f32; 3], max: [f32; 3]) -> Llsd {
        let vec = |value: [f32; 3]| {
            Llsd::Array(
                value
                    .into_iter()
                    .map(|c| Llsd::Real(f64::from(c)))
                    .collect(),
            )
        };
        Llsd::Map(HashMap::from([
            ("Min".to_owned(), vec(min)),
            ("Max".to_owned(), vec(max)),
        ]))
    }

    /// A fetcher that serves a fixed whole asset and counts its calls.
    #[derive(Debug)]
    struct CountingFetcher {
        /// The whole asset bytes returned for any range (a `200` response).
        asset: Bytes,
        /// Shared count of `fetch_range` calls.
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AssetFetcher<MeshKey> for CountingFetcher {
        async fn fetch_range(
            &self,
            _id: MeshKey,
            _start: usize,
            _end: usize,
        ) -> Result<FetchChunk, FetchError> {
            let _previous = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(FetchChunk {
                bytes: self.asset.clone(),
                whole: true,
            })
        }
    }

    /// A store over a counting fetcher (no disk cache), plus the call counter.
    fn store_with(asset: Bytes) -> Result<(MeshStore, Arc<AtomicUsize>), TestError> {
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher: Arc<dyn MeshFetcher> = Arc::new(CountingFetcher {
            asset,
            calls: Arc::clone(&calls),
        });
        let store = MeshStore::new(fetcher, None, CacheLimits::default())?;
        Ok((store, calls))
    }

    #[test]
    fn fetches_decodes_and_caches_one_mesh() -> Result<(), TestError> {
        let asset = Bytes::from(synth_mesh()?);
        let (store, calls) = store_with(asset)?;
        let id = MeshKey::from(Uuid::from_u128(1));
        pollster::block_on(async {
            let entry = store.get(id, MeshLod::High).await?;
            let mesh = entry.mesh().ok_or("decoded geometry")?;
            assert_eq!(mesh.lod, MeshLod::High);
            assert_eq!(mesh.submeshes.len(), 1);
            assert_eq!(mesh.vertex_count(), 3);
            assert_eq!(mesh.triangle_count(), 1);
            // A second get returns the same shared entry from memory (no re-fetch).
            let again = store.get(id, MeshLod::High).await?;
            assert!(Arc::ptr_eq(&entry, &again));
            Ok::<(), TestError>(())
        })?;
        // The whole asset came back on the first fetch, so all blocks are in the
        // asset region: header + LOD were served from that one response.
        assert_eq!(calls.load(Ordering::SeqCst), 1, "single-flight whole fetch");
        Ok(())
    }

    #[test]
    fn set_lod_reloads_a_different_level() -> Result<(), TestError> {
        let asset = Bytes::from(synth_mesh()?);
        let (store, _calls) = store_with(asset)?;
        let id = MeshKey::from(Uuid::from_u128(2));
        pollster::block_on(async {
            let entry = store.get(id, MeshLod::High).await?;
            assert_eq!(entry.current_lod(), Some(MeshLod::High));
            // Only high_lod exists, so a request for Low falls back to High.
            store.set_lod(&entry, MeshLod::Low).await?;
            assert_eq!(entry.current_lod(), Some(MeshLod::High));
            Ok::<(), TestError>(())
        })
    }

    #[test]
    fn weak_references_collect_after_drop() -> Result<(), TestError> {
        let asset = Bytes::from(synth_mesh()?);
        let (store, _calls) = store_with(asset)?;
        let id = MeshKey::from(Uuid::from_u128(3));
        pollster::block_on(async {
            let entry = store.get(id, MeshLod::High).await?;
            assert!(store.peek(id).is_some());
            drop(entry);
            store.sweep();
            assert!(store.peek(id).is_none());
            Ok::<(), TestError>(())
        })
    }

    #[test]
    fn disk_cache_avoids_a_second_network_fetch() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-mesh-store-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let asset = Bytes::from(synth_mesh()?);
        let calls = Arc::new(AtomicUsize::new(0));
        let fetcher: Arc<dyn MeshFetcher> = Arc::new(CountingFetcher {
            asset,
            calls: Arc::clone(&calls),
        });
        let store = MeshStore::new(fetcher, Some(dir.clone()), CacheLimits::default())?;
        let id = MeshKey::from(Uuid::from_u128(4));
        pollster::block_on(async {
            let _first = store.get(id, MeshLod::High).await?;
            store.sweep();
            // A fresh store on the same dir reads the cached asset, no network.
            let store2 = MeshStore::new(
                Arc::new(CountingFetcher {
                    asset: Bytes::new(),
                    calls: Arc::clone(&calls),
                }),
                Some(dir.clone()),
                CacheLimits::default(),
            )?;
            let entry = store2.get(id, MeshLod::High).await?;
            assert_eq!(entry.current_lod(), Some(MeshLod::High));
            Ok::<(), TestError>(())
        })?;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second store served from disk"
        );
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn get_publishes_ready_progress() -> Result<(), TestError> {
        use crate::progress::MeshProgress;
        let asset = Bytes::from(synth_mesh()?);
        let (store, _calls) = store_with(asset)?;
        let id = MeshKey::from(Uuid::from_u128(7));
        pollster::block_on(async {
            let entry = store.get(id, MeshLod::High).await?;
            // `get` must leave the entry's observable progress truthful (Ready),
            // not stuck at the mid-flight Downloading / Decoding it passed through.
            assert_eq!(entry.progress(), MeshProgress::Ready(MeshLod::High));
            Ok::<(), TestError>(())
        })
    }

    #[test]
    fn stats_bucket_a_ready_mesh_and_count_collection() -> Result<(), TestError> {
        use crate::progress::Priority;
        let asset = Bytes::from(synth_mesh()?);
        let (store, _calls) = store_with(asset)?;
        let id = MeshKey::from(Uuid::from_u128(6));
        pollster::block_on(async {
            let request = store.request(id, MeshLod::High, Priority::new(1));
            let entry = request.resolved().await?;
            // One ready entry, held in memory, with a non-zero asset footprint.
            // (The request/drive path publishes the terminal Ready progress.)
            let ready = store.stats();
            assert_eq!(ready.in_memory, 1);
            assert_eq!(ready.ready, 1);
            assert!(ready.bytes > 0);
            assert_eq!(ready.collected, 0);
            // Dropping the entry and the request and sweeping collects it.
            drop(entry);
            drop(request);
            store.sweep();
            let swept = store.stats();
            assert_eq!(swept.in_memory, 0);
            assert_eq!(swept.collected, 1);
            Ok::<(), TestError>(())
        })
    }

    #[test]
    fn missing_mesh_reports_not_a_mesh() -> Result<(), TestError> {
        let (store, _calls) = store_with(Bytes::from_static(b"not a mesh asset"))?;
        let id = MeshKey::from(Uuid::from_u128(5));
        pollster::block_on(async {
            let outcome = store.get(id, MeshLod::High).await;
            assert!(matches!(outcome, Err(MeshError::NotAMesh)));
            Ok::<(), TestError>(())
        })
    }
}
