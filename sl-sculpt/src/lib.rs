//! Pure sculpt-texture tessellation for Second Life / OpenSim clients — the
//! sculpt counterpart of `sl-prim` and `sl-mesh`.
//!
//! See the crate `README.md` for an overview. A decoded RGB sculpt map
//! ([`sl_texture::DecodedImage`]) is read as a displacement grid and stitched
//! into geometry (reusing `sl-prim`'s [`PrimMesh`] / [`PrimFace`] output type)
//! in Second Life's right-handed **Z-up** space. It is deliberately Bevy-free
//! and I/O-free — it never fetches or decodes; the caller sources the decoded
//! map from the shared `sl-texture` `TextureStore` and the `to_bevy_prim_mesh`
//! conversion lives in `sl-client-bevy`.
//!
//! The two pieces are:
//!
//! - [`stitch`] — the [`SculptStitch`] topology and its [`SculptParams`] flags,
//!   parsed from the wire `sculpt_type` byte.
//! - [`tessellate`](mod@tessellate) — the resample-and-stitch that turns a
//!   sculpt map into a single-face [`PrimMesh`], honouring the four sculpt
//!   types (plane / cylinder / sphere / torus) and the mirror / invert flags,
//!   following Firestorm's `LLVolume::sculpt` / `sculptGenerateMapVertices`,
//!   reimplemented idiomatically.

pub mod stitch;
pub mod tessellate;

pub use sl_prim::{PrimFace, PrimFaceId, PrimMesh};
pub use sl_texture::DecodedImage;
pub use stitch::{SculptParams, SculptStitch};
pub use tessellate::{WORKING_SUBDIVISIONS, tessellate, tessellate_with};
