//! Generic, level-of-detail-agnostic asset fetch scheduling primitives shared by
//! the Second Life / OpenSim asset stores (`sl-texture`, `sl-mesh`).
//!
//! Everything here is deliberately domain-free — it knows nothing about
//! textures, meshes, discard levels, or mesh LODs. Each higher-level store keeps
//! its own concrete entry, decoded representation, disk cache, and LOD flow, and
//! builds them on top of these shared pieces:
//!
//! - [`priority`] — the opaque [`Priority`] urgency and the diminishing
//!   [`popularity_boost`].
//! - [`gate`] — the bounded, priority-ordered [`PriorityGate`] admission gate.
//! - [`fetcher`] — the runtime-agnostic [`AssetFetcher`] network abstraction.
//! - [`cpu`] — the [`run_cpu`] rayon bridge for CPU-bound decode/downsample work.
//!
//! The per-store `Requesters` set and progress enums are *not* here — they carry
//! the store's LOD type, so each store defines its own (each calling the shared
//! [`popularity_boost`]).

pub mod cpu;
pub mod fetcher;
pub mod gate;
pub mod priority;

pub use cpu::run_cpu;
pub use fetcher::{AssetFetcher, FetchChunk, FetchError};
pub use gate::{GatePermit, PriorityGate};
pub use priority::{POPULARITY_BOOST_SCALE, Priority, popularity_boost};
