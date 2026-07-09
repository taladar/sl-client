//! The shared mesh pipeline: fetch, off-thread decode, and disk-cache every mesh
//! asset the scene needs through the LOD-aware [`MeshStore`], then hand the
//! decoded geometry to the object system that spawns its submesh entities.
//!
//! This is the mesh counterpart of [`textures`](crate::textures): rather than
//! decode an `LLMesh` asset on the render thread, the viewer drives the same
//! store the headless client uses. A [`BevyMeshFetcher`] pulls `GetMesh2` /
//! `GetMesh` asset byte ranges over blocking HTTP on Bevy's [`IoTaskPool`], the
//! store decodes them on its own `rayon` pool, keeps a Firestorm-compatible
//! per-UUID `.mesh` on-disk cache (so a mesh survives across runs), and dedupes
//! concurrent requests for the same mesh. [`MeshManager`] owns that store; each
//! mesh is fetched through a background [`Task`], and [`poll_meshes`] folds a
//! completed decode into the shared cache and announces it with a [`MeshDecoded`]
//! message the [`objects`](crate::objects) system reacts to (building one child
//! entity per submesh and texturing it via the Phase 6 texture pipeline).
//!
//! Built in the Phase 7 slice (mesh geometry) and extended with pixel-area
//! level-of-detail selection in P21.2: an ordinary scene mesh is fetched at a
//! coarse placeholder block and upgraded / downgraded toward the level its
//! on-screen size warrants (a worn attachment is boosted to the finest level and
//! left there, mirroring the boosted-texture rule of P21.1). The low-level
//! `Command::FetchMesh` / `MeshReceived` path is deliberately *not* used,
//! mirroring the Phase 6 texture work that moved off the equivalent raw texture
//! path.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::{
    BevyMeshFetcher, CAP_GET_MESH, CAP_GET_MESH2, DecodedMesh, GateStats, MeshCacheLimits,
    MeshFetcher, MeshKey, MeshLod, MeshRequest, MeshSkin, MeshStore, Priority, SlCapabilities,
    StoreStats,
};

/// The outcome of one background mesh fetch: the decoded geometry paired with the
/// decoded rig skin (`None` when the mesh carries no skin block), or `None` for
/// the whole fetch if the geometry could not be fetched or decoded.
type FetchResult = Option<(Arc<DecodedMesh>, Option<Arc<MeshSkin>>)>;

/// The outcome of one background mesh level-of-detail change (P21.2): the newly
/// decoded geometry block, or `None` if the swap could not be fetched or decoded
/// (the mesh then keeps the level it already had). Only the geometry changes on a
/// LOD swap — the rig skin is level-independent, so it is not re-decoded.
type LodResult = Option<Arc<DecodedMesh>>;

/// The mesh level of detail a pixel-area-managed mesh (P21.2) is first requested
/// at, before its on-screen size is known: a coarse-but-quick placeholder block
/// that loads fast, which the render-priority driver then upgrades / downgrades to
/// the level the object's apparent size warrants. This mirrors the coarse-first
/// texture placeholder (P21.1) — a small / distant mesh stays coarse and only the
/// fidelity the view warrants is fetched.
const INITIAL_MANAGED_LOD: MeshLod = MeshLod::Low;

/// The per-mesh level-of-detail state of a pixel-area-managed mesh (P21.2). Its
/// presence in [`MeshManager::managed`] marks the mesh as LOD-managed (an ordinary
/// scene mesh object); boosted meshes (worn attachments, whose skinned transform
/// the pixel-area pass cannot rank) are absent and stay at the finest level.
struct ManagedMeshLod {
    /// The level of the currently decoded geometry. The render-priority driver
    /// compares its target against this to decide whether to upgrade, downgrade,
    /// or leave the mesh unchanged; it is updated from the actually decoded block
    /// once each fetch / LOD change completes.
    current: MeshLod,
}

/// Announced (once per mesh id) when a background fetch finishes — whether it
/// decoded or failed. The object system reads this and either builds the now-
/// cached submesh geometry or leaves the object geometry-less.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct MeshDecoded(pub(crate) MeshKey);

/// The shared mesh fetch/decode/cache pipeline: one [`MeshStore`] plus the
/// in-flight background fetch tasks and the decoded meshes already in hand.
///
/// The object system calls [`request`](Self::request) to ensure a mesh is being
/// fetched, then — once a [`MeshDecoded`] names it — reads [`decoded`](Self::decoded)
/// for the geometry to spawn.
#[derive(Resource)]
pub(crate) struct MeshManager {
    /// The LOD-aware store doing the fetch, off-thread decode, dedupe, and on-disk
    /// caching.
    store: MeshStore,
    /// The store's HTTP fetcher, kept here so its `GetMesh2` / `GetMesh`
    /// capability URL can be refreshed as the agent changes region.
    fetcher: Arc<BevyMeshFetcher>,
    /// The background fetch task per mesh id, polled to completion by
    /// [`poll_meshes`]; presence means "already being fetched".
    inflight: HashMap<MeshKey, Task<FetchResult>>,
    /// The re-prioritizable request handle per in-flight mesh id (P20.2), paired
    /// with the request-time (base) priority it was issued at, so the
    /// render-priority driver can raise a mesh the camera looks at (via
    /// [`set_priority`](Self::set_priority)) while it is still queued behind the
    /// store's admission gate — but never *demote* a boosted request (a worn
    /// avatar attachment) below its base. Cleared alongside
    /// [`inflight`](Self::inflight) once the fetch resolves.
    requests: HashMap<MeshKey, (MeshRequest, Priority)>,
    /// Per-mesh level-of-detail state for pixel-area-managed meshes (P21.2), keyed
    /// by mesh id. Presence marks a mesh as LOD-managed; the render-priority driver
    /// upgrades / downgrades it toward the level the owning object's on-screen size
    /// warrants. The mesh's initial [`MeshRequest`] handle is retained in
    /// [`requests`](Self::requests) (rather than dropped on resolve) for exactly
    /// these, so its store entry stays live for [`MeshStore::set_lod`].
    managed: HashMap<MeshKey, ManagedMeshLod>,
    /// In-flight level-of-detail changes (P21.2), one per mesh, kept separate from
    /// the initial [`inflight`](Self::inflight) fetch so a re-decode never blocks
    /// (or is mistaken for) the first fetch and at most one LOD change runs per
    /// mesh at a time.
    lod_inflight: HashMap<MeshKey, Task<LodResult>>,
    /// Successfully decoded meshes by id, shared across all consumers so a mesh is
    /// fetched and decoded once no matter how many objects use it.
    decoded: HashMap<MeshKey, Arc<DecodedMesh>>,
    /// The decoded rig skin of each mesh that carries one (P17.2), shared like
    /// [`decoded`](Self::decoded); absent for a mesh with no skin block.
    skins: HashMap<MeshKey, Arc<MeshSkin>>,
    /// Requests made before the region's mesh capability was known, held here (at
    /// their base priority) instead of failed. A fetch issued before the seed caps
    /// arrive would fail permanently ("mesh capability not available"); these are
    /// drained and issued for real by [`retry_pending`](Self::retry_pending) once
    /// the cap is set. (Object updates normally arrive after the caps, so this is a
    /// latent-race guard rather than a routinely-hit path.)
    pending: HashMap<MeshKey, Priority>,
}

impl FromWorld for MeshManager {
    /// Build the store over a fresh [`BevyMeshFetcher`], backed by the on-disk
    /// mesh cache when a cache directory is available (falling back to an
    /// in-memory-only store if the cache cannot be opened).
    fn from_world(_world: &mut World) -> Self {
        let fetcher = Arc::new(BevyMeshFetcher::new());
        let disk_dir = mesh_cache_dir();
        let store = build_store(&fetcher, disk_dir);
        Self {
            store,
            fetcher,
            inflight: HashMap::new(),
            requests: HashMap::new(),
            managed: HashMap::new(),
            lod_inflight: HashMap::new(),
            decoded: HashMap::new(),
            skins: HashMap::new(),
            pending: HashMap::new(),
        }
    }
}

impl MeshManager {
    /// Ensure `id` is being fetched at request-time (base) priority `priority`:
    /// spawn a background fetch task if the mesh is not already decoded or in
    /// flight. A nil id (no mesh) is ignored.
    ///
    /// An ordinary scene mesh is requested at [`Priority::IDLE`] and the
    /// render-priority driver ([`drive_render_priority`]) raises it each throttled
    /// frame from the owning object's on-screen pixel area (P20.2), so a mesh the
    /// camera looks at loads ahead of a distant one; a worn avatar attachment is
    /// requested at a boost (its skinned entity transform does not reflect its
    /// on-screen size, so the pixel-area pass cannot rank it — the base priority,
    /// which the driver never demotes below, is what keeps it ahead).
    ///
    /// An ordinary (unboosted) scene mesh is also **pixel-area LOD managed**
    /// (P21.2): it is first fetched at a coarse [placeholder level](INITIAL_MANAGED_LOD)
    /// and the render-priority driver then upgrades / downgrades it via
    /// [`set_lod_for_area`](Self::set_lod_for_area) to the level the owning object's
    /// on-screen size warrants, so a small / distant mesh fetches only a coarse
    /// block. A boosted mesh (a worn attachment, whose skinned transform the
    /// pixel-area pass cannot rank) is instead fetched at [`MeshLod::FINEST`] and
    /// left there — mirroring the boosted-texture rule (P21.1).
    ///
    /// Idempotent — many objects sharing the same mesh trigger a single fetch, on
    /// top of the store's own single-flight dedupe.
    ///
    /// [`drive_render_priority`]: crate::render_priority::drive_render_priority
    pub(crate) fn request(&mut self, id: MeshKey, priority: Priority) {
        if id.uuid().is_nil() || self.decoded.contains_key(&id) || self.inflight.contains_key(&id) {
            return;
        }
        // The fetch needs the region's mesh capability. If it is not set yet (a
        // rare race — object updates usually arrive after the seed caps), hold the
        // request rather than spawn a fetch that would fail for good;
        // `retry_pending` issues it once the cap is up.
        if !self.fetcher.has_cap_url() {
            let _previous = self.pending.insert(id, priority);
            return;
        }
        self.pending.remove(&id);
        // A boosted mesh (a worn attachment) loads at full detail and is not LOD
        // managed; an ordinary scene mesh starts at a coarse placeholder block the
        // driver then refines (P21.2).
        let managed = !crate::render_priority::is_boost_priority(priority);
        let target = if managed {
            INITIAL_MANAGED_LOD
        } else {
            MeshLod::FINEST
        };
        let request = self.store.request(id, target, priority);
        let task_request = request.clone();
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            // The blocking `GetMesh` fetch runs on this IoTaskPool thread once the
            // request is admitted through the gate (in priority order); the decode
            // is dispatched onto the store's own CPU pool, so the render thread
            // never decodes.
            let geometry = match task_request.resolved().await {
                Ok(entry) => entry.mesh(),
                Err(_error) => None,
            }?;
            // Also decode the rig skin so a worn rigged mesh can be bound to the
            // avatar skeleton (P17.2); a mesh with no skin block yields `None`
            // here without failing the geometry fetch.
            let skin = match store.get_skin(id).await {
                Ok(entry) => entry.skin(),
                Err(_error) => None,
            };
            Some((geometry, skin))
        });
        let _previous = self.requests.insert(id, (request, priority));
        if managed {
            // Register for pixel-area LOD management (P21.2); the retained request
            // handle keeps its store entry live for later `set_lod`.
            let _existing = self
                .managed
                .entry(id)
                .or_insert(ManagedMeshLod { current: target });
        }
        self.inflight.insert(id, task);
    }

    /// Upgrade or downgrade a pixel-area-managed mesh (P21.2) toward `desired` —
    /// the level of detail the owning object's on-screen size warrants
    /// ([`MeshLod::for_distance`]) — via [`MeshStore::set_lod`]. Called by the
    /// render-priority driver each throttled frame.
    ///
    /// A no-op unless the mesh is LOD-managed, the chosen level differs from the
    /// current one, no LOD change for it is already running, and its retained
    /// request handle is still live. The store's `set_lod` fetches + decodes the
    /// target geometry block (waiting for any GPU read-lease to release before it
    /// frees the old one). The completed block is folded in by [`poll_meshes`],
    /// which re-announces the mesh so the object system rebuilds its submesh
    /// entities from the new geometry.
    pub(crate) fn set_lod_for_area(&mut self, id: MeshKey, desired: MeshLod) {
        let Some(state) = self.managed.get(&id) else {
            return;
        };
        if desired == state.current || self.lod_inflight.contains_key(&id) {
            return;
        }
        let Some((request, _base)) = self.requests.get(&id) else {
            // The retained handle is what keeps the entry live; without it we
            // cannot drive a LOD change.
            return;
        };
        let entry = request.entry();
        let store = self.store.clone();
        debug!(
            "mesh {id} pixel-area LOD: {:?} -> {desired:?}",
            state.current
        );
        let task = IoTaskPool::get().spawn(async move {
            if let Err(error) = store.set_lod(&entry, desired).await {
                warn!("mesh {id} LOD change to {desired:?} failed: {error}");
            }
            entry.mesh()
        });
        let _previous = self.lod_inflight.insert(id, task);
    }

    /// Stop pixel-area LOD managing `id` and upgrade its geometry to
    /// [`MeshLod::FINEST`] — used when a mesh is discovered to be **rigged** (a
    /// worn avatar attachment, or an animesh) *after* it was already requested as
    /// an ordinary managed scene mesh, because its worn status was not yet known
    /// when the fetch began. A far / late-rezzing avatar's attachment can decode
    /// before its wearer link resolves, so its mesh is requested at
    /// [`Priority::IDLE`] and starts on the managed, coarse-block path; a rigged
    /// mesh's skinned transform cannot be ranked by the pixel-area pass, so — like
    /// any boosted worn attachment — it must render at the finest block and never
    /// be LOD reduced. A no-op if the mesh is not (or no longer) managed, which
    /// includes the common case where its worn status was known up front and it
    /// was already fetched boosted at finest.
    pub(crate) fn upgrade_to_finest(&mut self, id: MeshKey) {
        if self.managed.remove(&id).is_none() {
            return;
        }
        let Some((request, _base)) = self.requests.get(&id) else {
            return;
        };
        let entry = request.entry();
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            if let Err(error) = store.set_lod(&entry, MeshLod::FINEST).await {
                warn!("mesh {id} upgrade to finest failed: {error}");
            }
            entry.mesh()
        });
        // Supersedes any coarser LOD change still queued for this mesh.
        let _previous = self.lod_inflight.insert(id, task);
    }

    /// Re-rank an in-flight mesh request from the on-screen pixel area the driver
    /// computed (P20.2), clamped to never fall below the request-time base
    /// priority — so the per-frame object pass can raise a mesh the camera turns
    /// toward, but cannot demote a boosted worn attachment (whose skinned entity
    /// transform the pixel-area pass ranks too low). A no-op for a mesh already
    /// decoded, never requested, or whose fetch already finished (its handle is
    /// dropped once it resolves).
    pub(crate) fn set_priority(&self, id: MeshKey, priority: Priority) {
        if let Some((request, base)) = self.requests.get(&id) {
            request.set_priority(Priority::combine(priority, *base));
        }
    }

    /// The decoded geometry for `id`, once it has been fetched, or `None` if it is
    /// still in flight or the fetch failed.
    pub(crate) fn decoded(&self, id: MeshKey) -> Option<&Arc<DecodedMesh>> {
        self.decoded.get(&id)
    }

    /// The decoded rig skin for `id`, once fetched, or `None` if the mesh carries
    /// no skin block, is still in flight, or the fetch failed (P17.2).
    pub(crate) fn skin(&self, id: MeshKey) -> Option<&Arc<MeshSkin>> {
        self.skins.get(&id)
    }

    /// The currently decoded level of detail of `id`, and whether it is pixel-area
    /// LOD managed (an ordinary scene mesh) rather than boosted to the finest level
    /// (a worn attachment) — for the crosshair pick tool's LOD diagnostics (P21.2).
    /// `None` if the mesh has not decoded yet.
    pub(crate) fn lod_debug(&self, id: MeshKey) -> Option<(MeshLod, bool)> {
        let mesh = self.decoded.get(&id)?;
        Some((mesh.lod, self.managed.contains_key(&id)))
    }

    /// Point the store's fetcher at the region's current mesh capability URL
    /// (`GetMesh2`, else `GetMesh`), or clear it when absent.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
    }

    /// Issue any requests that were made before the mesh capability was known (see
    /// [`pending`](Self::pending)), now that it is. A no-op while the cap is unset
    /// or nothing is pending. Call this whenever the cap is (re)set.
    pub(crate) fn retry_pending(&mut self) {
        if self.pending.is_empty() || !self.fetcher.has_cap_url() {
            return;
        }
        // Drain first, then re-issue: `request` removes each id from `pending` and
        // spawns its fetch now the cap resolves.
        let pending: Vec<(MeshKey, Priority)> = self.pending.drain().collect();
        for (id, priority) in pending {
            self.request(id, priority);
        }
    }

    /// A point-in-time snapshot of the mesh fetch/decode pipeline (P19.2), for
    /// the diagnostics overlay: entry counts bucketed by stage plus the
    /// cumulative disk-cache-hit / GC counters.
    pub(crate) fn stats(&self) -> StoreStats {
        self.store.stats()
    }

    /// A point-in-time snapshot of the mesh store's admission gate (P19.2): its
    /// concurrency capacity, in-flight slots, and queued waiters.
    pub(crate) fn gate_stats(&self) -> GateStats {
        self.store.gate_stats()
    }
}

/// Build a [`MeshStore`] over `fetcher`, backed by the on-disk cache at `disk_dir`
/// when it can be opened, and otherwise in-memory only (a disk-cache failure must
/// never keep the viewer from rendering).
fn build_store(fetcher: &Arc<BevyMeshFetcher>, disk_dir: Option<PathBuf>) -> MeshStore {
    // Coerce the concrete fetcher to the trait object the store stores (a move
    // through a typed binding, since `Arc::clone`'s inferred `T` would otherwise
    // demand the argument already be the trait object). The concrete `Arc` is kept
    // in the manager for `set_cap_url`.
    let concrete = Arc::clone(fetcher);
    let fetcher: Arc<dyn MeshFetcher> = concrete;
    if let Some(dir) = disk_dir {
        match MeshStore::new(Arc::clone(&fetcher), Some(dir), MeshCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("mesh disk cache unavailable ({error}); running in-memory only"),
        }
    }
    // The disk-less store opens no files and so cannot fail; the loop extracts it
    // without an `unwrap`/`expect` (which the lints forbid) and runs exactly once.
    loop {
        match MeshStore::new(Arc::clone(&fetcher), None, MeshCacheLimits::default()) {
            Ok(store) => return store,
            Err(error) => warn!("in-memory mesh store failed to open ({error}); retrying"),
        }
    }
}

/// The viewer's on-disk mesh cache directory (`<cache>/sl-client-bevy-viewer/
/// meshcache`), from `XDG_CACHE_HOME` or `~/.cache`, or `None` when neither is set
/// (the store then runs in-memory only).
fn mesh_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("sl-client-bevy-viewer").join("meshcache"))
}

/// Refresh the store fetcher's mesh capability URL each time the region's
/// capability map is (re)discovered, preferring `GetMesh2` over `GetMesh`.
pub(crate) fn update_mesh_caps(
    mut capabilities: MessageReader<SlCapabilities>,
    mut manager: ResMut<MeshManager>,
) {
    for SlCapabilities(map) in capabilities.read() {
        let url = map
            .get(CAP_GET_MESH2)
            .or_else(|| map.get(CAP_GET_MESH))
            .cloned();
        manager.set_cap_url(url);
    }
    // Issue any requests parked while the mesh cap was still unknown.
    manager.retry_pending();
}

/// Poll the in-flight fetch tasks; move each completed decode into the shared
/// cache and announce it with a [`MeshDecoded`] message (emitted on failure too,
/// so the object system can stop waiting on it).
pub(crate) fn poll_meshes(
    mut manager: ResMut<MeshManager>,
    mut decoded: MessageWriter<MeshDecoded>,
) {
    // Collect the ids whose task has finished, then apply — the borrow of the task
    // map cannot overlap the mutation of the decoded map.
    let mut finished: Vec<(MeshKey, FetchResult)> = Vec::new();
    for (&id, task) in &mut manager.inflight {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((id, result));
        }
    }
    for (id, result) in finished {
        let _removed = manager.inflight.remove(&id);
        // Drop the schedulable request handle now the fetch is done — the decoded
        // geometry lives in `decoded`, independent of the store entry (P20.2) —
        // *unless* the mesh is pixel-area LOD managed (P21.2), where the retained
        // handle keeps its store entry live for later `set_lod`.
        if !manager.managed.contains_key(&id) {
            let _request = manager.requests.remove(&id);
        }
        if let Some((mesh, skin)) = result {
            // Record the level actually decoded (the store may have fallen back to
            // a coarser available block than requested), so the driver ranks
            // further changes against the real current level (P21.2).
            if let Some(state) = manager.managed.get_mut(&id) {
                state.current = mesh.lod;
            }
            let _previous = manager.decoded.insert(id, mesh);
            if let Some(skin) = skin {
                let _prev_skin = manager.skins.insert(id, skin);
            }
        }
        decoded.write(MeshDecoded(id));
    }

    // Fold in completed level-of-detail changes (P21.2): the store entry now holds
    // the finer / coarser geometry block, so refresh the shared decoded cache and
    // re-announce the mesh so `apply_object_meshes` rebuilds its submesh entities.
    let mut lod_finished: Vec<(MeshKey, LodResult)> = Vec::new();
    for (&id, task) in &mut manager.lod_inflight {
        if let Some(result) = block_on(poll_once(task)) {
            lod_finished.push((id, result));
        }
    }
    for (id, result) in lod_finished {
        let _removed = manager.lod_inflight.remove(&id);
        if let Some(mesh) = result {
            if let Some(state) = manager.managed.get_mut(&id) {
                state.current = mesh.lod;
            }
            let _previous = manager.decoded.insert(id, mesh);
            decoded.write(MeshDecoded(id));
        }
    }
}
