//! A higher-level mesh fetch API, LLMesh decoder, and Firestorm-compatible
//! on-disk cache for Second Life / OpenSim clients — the mesh counterpart of
//! `sl-texture`, sharing its scheduling primitives via `sl-asset-sched`.
//!
//! See the crate `README.md` for an overview. Level-of-detail is expressed with
//! [`sl_proto::MeshLod`] (four discrete geometry blocks, unlike a texture's
//! progressive discard levels). The pieces are:
//!
//! - [`decode`] — LLMesh header parsing and geometry / skin / physics decoding.
//! - [`encode`] — its inverse: writing an LLMesh asset, which model upload and
//!   the mesh test fixtures both need.
//! - [`disk`] — the Firestorm-compatible per-UUID on-disk mesh cache.
//! - [`fetcher`] — the runtime-agnostic network abstraction.
//! - [`entry`] — the shared, LOD-aware mesh object and its geometry lease.
//! - [`store`] — the weak-reference fetch/decode/cache store.
//! - [`progress`] — priority, progress observation, and cancellation.

pub mod decode;
pub mod disk;
pub mod encode;
pub mod entry;
pub mod fetcher;
pub mod progress;
pub mod seams;
pub mod store;

pub use decode::{
    BlockRef, DecodedMesh, MeshDecodeError, MeshHeader, MeshPhysics, MeshSkin, PhysicsConvex,
    Submesh, VertexWeights, decode_lod, decode_physics_convex, decode_physics_mesh, decode_skin,
    parse_header,
};
pub use disk::{AssetBytes, CacheLimits, MeshDiskCache};
pub use encode::{
    MeshEncodeError, MeshModel, encode_lod_block, encode_mesh, encode_physics_convex_block,
    encode_skin_block,
};
pub use entry::{MeshEntry, MeshReadLease};
pub use fetcher::{AssetFetcher, FetchChunk, FetchError, MeshFetcher};
pub use progress::{MeshProgress, MeshRequest, Priority};
pub use seams::{SeamHit, UvSeamReport, analyze_uv_seams, uv_seam_hits};
pub use sl_asset_sched::{GateStats, StoreStats};
pub use store::{MeshError, MeshStore};

pub use sl_proto::{DEFAULT_LOD_FACTOR, MeshKey, MeshLod};
