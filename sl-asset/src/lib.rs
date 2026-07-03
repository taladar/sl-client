//! A higher-level generic-asset fetch API and Firestorm-compatible on-disk
//! cache for Second Life / OpenSim clients — the opaque-asset counterpart of
//! `sl-texture` and `sl-mesh`, sharing their scheduling primitives via
//! `sl-asset-sched`.
//!
//! Where those crates decode their asset class (JPEG-2000 → RGBA, LLMesh →
//! geometry) and manage level of detail, a *generic* asset (sound, animation,
//! landmark, notecard, gesture, body part, clothing, …) is an opaque blob the
//! client stores as-is. So this store is deliberately smaller: it fetches the
//! whole asset once over the `ViewerAsset` capability, de-duplicates concurrent
//! and repeat requests, and caches the bytes on disk — with no decode step and
//! no LOD flow. The pieces are:
//!
//! - [`disk`] — the Firestorm-compatible per-UUID on-disk asset cache.
//! - [`fetcher`] — the runtime-agnostic network abstraction and the
//!   `(id, class)` [`AssetRef`] key.
//! - [`entry`] — the shared, opaque asset object.
//! - [`store`] — the weak-reference fetch/cache store.
//! - [`progress`] — fetch progress observation and priority.

pub mod disk;
pub mod entry;
pub mod fetcher;
pub mod progress;
pub mod store;

pub use disk::{AssetDiskCache, CacheLimits};
pub use entry::AssetEntry;
pub use fetcher::{AssetFetcher, AssetRef, BlobFetcher, FetchChunk, FetchError};
pub use progress::{AssetProgress, Priority};
pub use store::{AssetError, AssetStore};

pub use sl_proto::{AssetKey, AssetType};
