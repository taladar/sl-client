//! The shared, level-of-detail-aware texture object the store hands out.
//!
//! A [`TextureEntry`] is held by consumers behind an `Arc`; the store keeps only
//! a `Weak` to it, so the texture becomes collectible when the last consumer
//! drops its `Arc` (pointer-count garbage collection). One entry represents one
//! logical texture across all levels of detail: its decoded image is *swapped in
//! place* on an upgrade (finer) or downgrade (coarser), never duplicated per
//! level. Reads are lock-free via `ArcSwap`; a per-entry usage lock lets a
//! downgrade wait until no pixels are leased to the GPU before it frees the
//! finer buffer.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use arc_swap::{ArcSwap, ArcSwapOption};
use bytes::Bytes;
use parking_lot::Mutex;
use sl_proto::{DiscardLevel, TextureKey, j2c};

use crate::decode::DecodedImage;
use crate::schedule::{Priority, Requesters, TextureProgress};

/// The JPEG-2000 codestream prefix fetched for a texture so far.
pub(crate) struct Codestream {
    /// The fetched leading bytes of the codestream.
    pub(crate) bytes: Bytes,
    /// Whether `bytes` is the entire asset (so no further fetch can grow it).
    pub(crate) complete: bool,
}

impl Codestream {
    /// An empty, not-yet-fetched codestream.
    const fn empty() -> Self {
        Self {
            bytes: Bytes::new(),
            complete: false,
        }
    }

    /// The number of codestream bytes fetched so far.
    pub(crate) const fn covered(&self) -> usize {
        self.bytes.len()
    }
}

/// One logical texture in the store: its fetched codestream, its current decoded
/// image (if any), and the locks coordinating level-of-detail changes.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `TextureEntry` reads clearly"
)]
pub struct TextureEntry {
    /// The texture's asset id.
    pub(crate) id: TextureKey,
    /// The parsed codestream header, cached once at least a header's worth of
    /// bytes has been fetched (`None` inside the lock means "not yet parsed").
    pub(crate) header: OnceLock<Option<j2c::Header>>,
    /// The codestream prefix fetched so far.
    pub(crate) codestream: ArcSwap<Codestream>,
    /// The current decoded image, or `None` before the first decode.
    pub(crate) image: ArcSwapOption<DecodedImage>,
    /// Serializes level-of-detail changes (single-writer / single-flight).
    pub(crate) write_lock: async_lock::Mutex<()>,
    /// Held for reading while pixels are leased/GPU-mapped; a downgrade takes it
    /// for writing so it only frees a finer buffer once nothing is using it.
    pub(crate) usage: async_lock::RwLock<()>,
    /// The current observable progress state.
    pub(crate) progress: ArcSwap<TextureProgress>,
    /// Signalled on every progress transition, to wake observers.
    pub(crate) progress_changed: event_listener::Event,
    /// The live requesters' `(priority, target)` contributions.
    pub(crate) requesters: Mutex<Requesters>,
    /// The combined (max) requester priority, cached for lock-free reads.
    pub(crate) effective_priority: AtomicU32,
    /// The finest requester target (as a raw discard level), cached; `u8::MAX`
    /// means "no requester".
    pub(crate) target_want: AtomicU8,
}

impl std::fmt::Debug for TextureEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureEntry")
            .field("id", &self.id)
            .field("discard_level", &self.current_discard())
            .finish_non_exhaustive()
    }
}

impl TextureEntry {
    /// A fresh entry with no codestream and no decoded image.
    pub(crate) fn new(id: TextureKey) -> Arc<Self> {
        Arc::new(Self {
            id,
            header: OnceLock::new(),
            codestream: ArcSwap::from_pointee(Codestream::empty()),
            image: ArcSwapOption::empty(),
            write_lock: async_lock::Mutex::new(()),
            usage: async_lock::RwLock::new(()),
            progress: ArcSwap::from_pointee(TextureProgress::Queued),
            progress_changed: event_listener::Event::new(),
            requesters: Mutex::new(Requesters::default()),
            effective_priority: AtomicU32::new(0),
            target_want: AtomicU8::new(u8::MAX),
        })
    }

    /// The current observable progress state.
    #[must_use]
    pub fn progress(&self) -> TextureProgress {
        **self.progress.load()
    }

    /// Publishes a new progress state and wakes observers if it changed.
    pub(crate) fn set_progress(&self, progress: TextureProgress) {
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

    /// The combined (max) priority across all current requesters.
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

    /// Registers a requester's `(priority, target)` contribution, refreshing the
    /// cached aggregates.
    pub(crate) fn add_requester(&self, id: u64, priority: Priority, target: DiscardLevel) {
        let mut requesters = self.requesters.lock();
        requesters.add(id, priority, target);
        self.refresh_aggregates(&requesters);
        drop(requesters);
    }

    /// Updates a requester's priority, refreshing the cached aggregates.
    pub(crate) fn set_requester_priority(&self, id: u64, priority: Priority) {
        let mut requesters = self.requesters.lock();
        requesters.set_priority(id, priority);
        self.refresh_aggregates(&requesters);
        drop(requesters);
    }

    /// Removes a requester, refreshing the aggregates and returning how many
    /// requesters remain.
    pub(crate) fn remove_requester(&self, id: u64) -> usize {
        let mut requesters = self.requesters.lock();
        requesters.remove(id);
        self.refresh_aggregates(&requesters);
        let remaining = requesters.len();
        drop(requesters);
        remaining
    }

    /// Recomputes and stores the cached `effective_priority`/`target_want` from
    /// the current requester set.
    fn refresh_aggregates(&self, requesters: &Requesters) {
        self.effective_priority
            .store(requesters.effective_priority().get(), Ordering::Release);
        let want = requesters
            .target_want()
            .map_or(u8::MAX, sl_proto::DiscardLevel::get);
        self.target_want.store(want, Ordering::Release);
    }

    /// The texture's asset id.
    #[must_use]
    pub const fn id(&self) -> TextureKey {
        self.id
    }

    /// The current decoded image, or `None` before the first decode. Cloning the
    /// returned `Arc` is cheap and pins the pixels until dropped.
    #[must_use]
    pub fn image(&self) -> Option<Arc<DecodedImage>> {
        self.image.load_full()
    }

    /// The level of detail of the current decoded image, if any.
    #[must_use]
    pub fn current_discard(&self) -> Option<DiscardLevel> {
        self.image.load().as_ref().map(|image| image.discard_level)
    }

    /// Leases the current pixels for reading (e.g. a GPU upload): holds the usage
    /// lock so a concurrent downgrade waits, and pins the pixel buffer via an
    /// `Arc`. Returns `None` if the texture has not been decoded yet.
    pub async fn lease(&self) -> Option<TextureReadLease<'_>> {
        let guard = self.usage.read().await;
        let image = self.image.load_full()?;
        Some(TextureReadLease {
            image,
            _guard: guard,
        })
    }

    /// Parses and caches the codestream header once enough bytes are present,
    /// returning it. Returns `None` while the codestream is too short to parse.
    pub(crate) fn header(&self) -> Option<j2c::Header> {
        if let Some(cached) = self.header.get() {
            return *cached;
        }
        let codestream = self.codestream.load();
        if codestream.covered() < j2c::FIRST_PACKET_SIZE && !codestream.complete {
            return j2c::parse_header(&codestream.bytes);
        }
        let parsed = j2c::parse_header(&codestream.bytes);
        let _stored = self.header.set(parsed);
        parsed
    }
}

/// A read lease on a texture's decoded pixels: keeps the pixel buffer alive and
/// blocks a concurrent downgrade until dropped. Obtained from
/// [`TextureEntry::lease`].
pub struct TextureReadLease<'entry> {
    /// The leased image, pinned for the lease's lifetime.
    image: Arc<DecodedImage>,
    /// The usage read guard held for the lease's lifetime.
    _guard: async_lock::RwLockReadGuard<'entry, ()>,
}

impl TextureReadLease<'_> {
    /// The leased decoded image.
    #[must_use]
    pub fn image(&self) -> &DecodedImage {
        &self.image
    }

    /// The leased RGBA8 pixels, ready for a zero-copy GPU upload.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.image.pixels
    }
}

impl std::fmt::Debug for TextureReadLease<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureReadLease")
            .field("discard_level", &self.image.discard_level)
            .finish_non_exhaustive()
    }
}
