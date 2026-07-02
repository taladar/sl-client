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

use std::sync::{Arc, OnceLock};

use arc_swap::{ArcSwap, ArcSwapOption};
use bytes::Bytes;
use sl_proto::{DiscardLevel, TextureKey, j2c};

use crate::decode::DecodedImage;

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
        })
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
