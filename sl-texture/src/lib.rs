//! A higher-level, level-of-detail-aware texture fetch API, decoding store, and
//! Firestorm-compatible on-disk cache for Second Life / OpenSim clients.
//!
//! See the crate `README.md` for an overview. The pieces are:
//!
//! Level-of-detail is expressed with [`sl_proto::DiscardLevel`].
//!
//! - [`decode`] — JPEG-2000 → RGBA8 decoding and pixel downsampling.
//!
//! Further modules (`disk`, `fetcher`, `entry`, `store`, `schedule`) are added
//! incrementally.

pub mod decode;

pub use decode::{DecodeError, DecodedImage, decode_j2c, downsample};
