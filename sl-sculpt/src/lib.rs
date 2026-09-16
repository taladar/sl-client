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
//! - [`tessellate`](mod@tessellate) — reading the map into a vertex grid and
//!   laying it over the prim's own path and profile (built by `sl-prim` at the
//!   sizes the map asks for), following Firestorm's `LLVolume::sculpt` /
//!   `sculptGenerateMapVertices`, reimplemented idiomatically. The grid is sized
//!   by [`mesh_resolution`] from the map's dimensions and the requested
//!   [`PrimLod`](sl_prim::PrimLod), as the reference sizes it from the volume's
//!   detail — a distant sculpt is not tessellated at full rez. On the usual
//!   circle-on-circle shape the result is one face; on another shape it is that
//!   shape's faces, as in the reference.

pub mod stitch;
pub mod tessellate;

pub use sl_prim::{PrimFace, PrimFaceId, PrimMesh};
pub use sl_texture::DecodedImage;
pub use stitch::{SculptParams, SculptStitch};
pub use tessellate::{MAX_SUBDIVISIONS, mesh_resolution, tessellate, tessellate_with};
