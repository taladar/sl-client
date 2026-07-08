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
//! This is the Phase 7 slice — mesh geometry at the finest level of detail. The
//! low-level `Command::FetchMesh` / `MeshReceived` path is deliberately *not*
//! used, mirroring the Phase 6 texture work that moved off the equivalent raw
//! texture path.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::{
    BevyMeshFetcher, CAP_GET_MESH, CAP_GET_MESH2, DecodedMesh, GateStats, MeshCacheLimits,
    MeshFetcher, MeshKey, MeshLod, MeshSkin, MeshStore, SlCapabilities, StoreStats,
};

/// The outcome of one background mesh fetch: the decoded geometry paired with the
/// decoded rig skin (`None` when the mesh carries no skin block), or `None` for
/// the whole fetch if the geometry could not be fetched or decoded.
type FetchResult = Option<(Arc<DecodedMesh>, Option<Arc<MeshSkin>>)>;

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
    /// Successfully decoded meshes by id, shared across all consumers so a mesh is
    /// fetched and decoded once no matter how many objects use it.
    decoded: HashMap<MeshKey, Arc<DecodedMesh>>,
    /// The decoded rig skin of each mesh that carries one (P17.2), shared like
    /// [`decoded`](Self::decoded); absent for a mesh with no skin block.
    skins: HashMap<MeshKey, Arc<MeshSkin>>,
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
            decoded: HashMap::new(),
            skins: HashMap::new(),
        }
    }
}

impl MeshManager {
    /// Ensure `id` is being fetched: spawn a background fetch task if the mesh is
    /// not already decoded or in flight. A nil id (no mesh) is ignored.
    ///
    /// Idempotent — many objects sharing the same mesh trigger a single fetch, on
    /// top of the store's own single-flight dedupe.
    pub(crate) fn request(&mut self, id: MeshKey) {
        if id.uuid().is_nil() || self.decoded.contains_key(&id) || self.inflight.contains_key(&id) {
            return;
        }
        let store = self.store.clone();
        let task = IoTaskPool::get().spawn(async move {
            // The blocking `GetMesh` fetch runs on this IoTaskPool thread; the
            // decode is dispatched onto the store's own CPU pool, so the render
            // thread never decodes.
            let geometry = match store.get(id, MeshLod::FINEST).await {
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
        self.inflight.insert(id, task);
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

    /// Point the store's fetcher at the region's current mesh capability URL
    /// (`GetMesh2`, else `GetMesh`), or clear it when absent.
    fn set_cap_url(&self, url: Option<String>) {
        self.fetcher.set_cap_url(url);
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
    manager: Res<MeshManager>,
) {
    for SlCapabilities(map) in capabilities.read() {
        let url = map
            .get(CAP_GET_MESH2)
            .or_else(|| map.get(CAP_GET_MESH))
            .cloned();
        manager.set_cap_url(url);
    }
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
        if let Some((mesh, skin)) = result {
            let _previous = manager.decoded.insert(id, mesh);
            if let Some(skin) = skin {
                let _prev_skin = manager.skins.insert(id, skin);
            }
        }
        decoded.write(MeshDecoded(id));
    }
}
