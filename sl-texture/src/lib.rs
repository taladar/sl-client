//! A higher-level, level-of-detail-aware texture fetch API, decoding store, and
//! Firestorm-compatible on-disk cache for Second Life / OpenSim clients.
//!
//! See the crate `README.md` for an overview. The pieces are:
//!
//! Level-of-detail is expressed with [`sl_proto::DiscardLevel`].
//!
//! - [`decode`] — JPEG-2000 → RGBA8 decoding and pixel downsampling.
//! - [`disk`] — the Firestorm-compatible on-disk texture cache.
//! - [`fetcher`] — the runtime-agnostic network abstraction.
//! - [`entry`] — the shared, LOD-aware texture object and its pixel lease.
//! - [`store`] — the weak-reference fetch/decode/cache store.
//! - [`schedule`] — priority, progress observation, and cancellation.

pub mod decode;
pub mod disk;
pub mod entry;
pub mod fetcher;
pub mod schedule;
pub mod store;

pub use decode::{DecodeError, DecodedImage, decode_j2c, downsample};
pub use disk::{CacheLimits, TextureDiskCache};
pub use entry::{TextureEntry, TextureReadLease};
pub use fetcher::{FetchChunk, FetchError, TextureFetcher};
pub use schedule::{Priority, TextureProgress, TextureRequest};
pub use store::{TextureError, TextureStore};
