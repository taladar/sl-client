//! The shared, level-of-detail-aware mesh object the store hands out.
//!
//! A [`MeshEntry`] is held by consumers behind an `Arc`; the store keeps only a
//! `Weak` to it, so the mesh becomes collectible when the last consumer drops
//! its `Arc` (pointer-count garbage collection). One entry represents one
//! logical mesh: the fetched asset region, the currently decoded geometry (at
//! one [`MeshLod`]), and the lazily decoded skin / physics. Reads are lock-free
//! via `ArcSwap`; a per-entry usage lock lets a level change wait until no
//! geometry is leased to the GPU before it swaps a different level in.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use arc_swap::{ArcSwap, ArcSwapOption};

use sl_proto::{MeshKey, MeshLod};

use crate::decode::{DecodedMesh, MeshHeader, MeshPhysics, MeshSkin};
use crate::disk::AssetBytes;
use crate::progress::{MeshProgress, Priority, Requesters};

/// The `target_want` sentinel meaning "no live requester".
const NO_TARGET: u8 = u8::MAX;

/// One logical mesh in the store: its fetched asset bytes, its currently decoded
/// geometry (if any), its lazily decoded skin / physics, and the locks
/// coordinating level-of-detail changes.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `MeshEntry` reads clearly"
)]
pub struct MeshEntry {
    /// The mesh's asset id.
    pub(crate) id: MeshKey,
    /// The parsed header, cached once the header bytes are fetched (`None` inside
    /// the lock means "not a recognisable mesh header").
    pub(crate) header: OnceLock<Option<MeshHeader>>,
    /// The fetched asset region (header + fetched blocks), or `None` before the
    /// header is fetched.
    pub(crate) asset: ArcSwapOption<AssetBytes>,
    /// The currently decoded geometry, or `None` before the first decode.
    pub(crate) lod: ArcSwapOption<DecodedMesh>,
    /// The decoded skin, decoded lazily once (LOD-independent).
    pub(crate) skin: ArcSwapOption<MeshSkin>,
    /// The decoded physics, decoded lazily once (LOD-independent).
    pub(crate) physics: ArcSwapOption<MeshPhysics>,
    /// Serializes level-of-detail changes (single-writer / single-flight).
    pub(crate) write_lock: async_lock::Mutex<()>,
    /// Held for reading while geometry is leased; a level change takes it for
    /// writing so it only swaps a different level in once nothing is using it.
    pub(crate) usage: async_lock::RwLock<()>,
    /// The current observable progress state.
    pub(crate) progress: ArcSwap<MeshProgress>,
    /// Signalled on every progress transition, to wake observers.
    pub(crate) progress_changed: event_listener::Event,
    /// The live requesters' `(priority, target)` contributions.
    pub(crate) requesters: parking_lot::Mutex<Requesters>,
    /// The combined effective requester priority, cached for lock-free reads.
    pub(crate) effective_priority: AtomicU32,
    /// The finest requester target (as a [`MeshLod::index`]), cached;
    /// [`NO_TARGET`] means "no requester".
    pub(crate) target_want: AtomicU8,
}

impl std::fmt::Debug for MeshEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshEntry")
            .field("id", &self.id)
            .field("lod", &self.current_lod())
            .finish_non_exhaustive()
    }
}

impl MeshEntry {
    /// A fresh entry with no asset and no decoded geometry.
    pub(crate) fn new(id: MeshKey) -> Arc<Self> {
        Arc::new(Self {
            id,
            header: OnceLock::new(),
            asset: ArcSwapOption::empty(),
            lod: ArcSwapOption::empty(),
            skin: ArcSwapOption::empty(),
            physics: ArcSwapOption::empty(),
            write_lock: async_lock::Mutex::new(()),
            usage: async_lock::RwLock::new(()),
            progress: ArcSwap::from_pointee(MeshProgress::Queued),
            progress_changed: event_listener::Event::new(),
            requesters: parking_lot::Mutex::new(Requesters::default()),
            effective_priority: AtomicU32::new(0),
            target_want: AtomicU8::new(NO_TARGET),
        })
    }

    /// The current observable progress state.
    #[must_use]
    pub fn progress(&self) -> MeshProgress {
        **self.progress.load()
    }

    /// Publishes a new progress state and wakes observers if it changed.
    pub(crate) fn set_progress(&self, progress: MeshProgress) {
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

    /// The combined effective priority across all current requesters.
    #[must_use]
    pub(crate) fn effective_priority(&self) -> Priority {
        Priority::new(self.effective_priority.load(Ordering::Acquire))
    }

    /// The number of live requesters (interest); `0` means collectible/cancelled.
    #[must_use]
    pub(crate) fn interest(&self) -> usize {
        let requesters = self.requesters.lock();
        let count = requesters.len();
        drop(requesters);
        count
    }

    /// Registers a requester's `(priority, target)` contribution.
    pub(crate) fn add_requester(&self, id: u64, priority: Priority, target: MeshLod) {
        let mut requesters = self.requesters.lock();
        requesters.add(id, priority, target);
        self.refresh_aggregates(&requesters);
        drop(requesters);
    }

    /// Updates a requester's priority.
    pub(crate) fn set_requester_priority(&self, id: u64, priority: Priority) {
        let mut requesters = self.requesters.lock();
        requesters.set_priority(id, priority);
        self.refresh_aggregates(&requesters);
        drop(requesters);
    }

    /// Removes a requester, returning how many remain.
    pub(crate) fn remove_requester(&self, id: u64) -> usize {
        let mut requesters = self.requesters.lock();
        requesters.remove(id);
        self.refresh_aggregates(&requesters);
        let remaining = requesters.len();
        drop(requesters);
        remaining
    }

    /// Recomputes the cached `effective_priority` / `target_want` aggregates.
    fn refresh_aggregates(&self, requesters: &Requesters) {
        self.effective_priority
            .store(requesters.effective_priority().get(), Ordering::Release);
        let want = requesters
            .target_want()
            .map_or(NO_TARGET, |lod| lod.index());
        self.target_want.store(want, Ordering::Release);
    }

    /// The finest level any live requester wants, or `None` when there are none.
    #[must_use]
    pub(crate) fn finest_want(&self) -> Option<MeshLod> {
        MeshLod::from_index(self.target_want.load(Ordering::Acquire))
    }

    /// The mesh's asset id.
    #[must_use]
    pub const fn id(&self) -> MeshKey {
        self.id
    }

    /// The current decoded geometry, or `None` before the first decode. Cloning
    /// the returned `Arc` is cheap and pins the geometry until dropped.
    #[must_use]
    pub fn mesh(&self) -> Option<Arc<DecodedMesh>> {
        self.lod.load_full()
    }

    /// The level of detail of the current decoded geometry, if any.
    #[must_use]
    pub fn current_lod(&self) -> Option<MeshLod> {
        self.lod.load().as_ref().map(|mesh| mesh.lod)
    }

    /// The decoded skin, if it has been fetched and decoded.
    #[must_use]
    pub fn skin(&self) -> Option<Arc<MeshSkin>> {
        self.skin.load_full()
    }

    /// The decoded physics, if it has been fetched and decoded.
    #[must_use]
    pub fn physics(&self) -> Option<Arc<MeshPhysics>> {
        self.physics.load_full()
    }

    /// The parsed header, if the header bytes have been fetched and parsed.
    #[must_use]
    pub fn header(&self) -> Option<MeshHeader> {
        self.header.get().copied().flatten()
    }

    /// Leases the current geometry for reading (e.g. a GPU upload): holds the
    /// usage lock so a concurrent level change waits, and pins the geometry.
    /// Returns `None` if the mesh has not been decoded yet.
    pub async fn lease(&self) -> Option<MeshReadLease<'_>> {
        let guard = self.usage.read().await;
        let mesh = self.lod.load_full()?;
        Some(MeshReadLease {
            mesh,
            _guard: guard,
        })
    }
}

/// A read lease on a mesh's decoded geometry: keeps the geometry alive and
/// blocks a concurrent level change until dropped. Obtained from
/// [`MeshEntry::lease`].
pub struct MeshReadLease<'entry> {
    /// The leased geometry, pinned for the lease's lifetime.
    mesh: Arc<DecodedMesh>,
    /// The usage read guard held for the lease's lifetime.
    _guard: async_lock::RwLockReadGuard<'entry, ()>,
}

impl MeshReadLease<'_> {
    /// The leased decoded mesh.
    #[must_use]
    pub fn mesh(&self) -> &DecodedMesh {
        &self.mesh
    }
}

impl std::fmt::Debug for MeshReadLease<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshReadLease")
            .field("lod", &self.mesh.lod)
            .finish_non_exhaustive()
    }
}
