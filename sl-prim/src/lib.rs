//! Pure Linden prim tessellation for Second Life / OpenSim clients — the
//! geometry counterpart of `sl-mesh` and `sl-texture`.
//!
//! See the crate `README.md` for an overview. Given a prim's shape parameters
//! this crate sweeps a 2D profile ring along an extrusion path and assembles
//! per-face geometry (positions, normals, texture coordinates, indices) in
//! Second Life's right-handed **Z-up** space. It is deliberately Bevy-free and
//! I/O-free; the `to_bevy_prim_mesh` conversion lives in `sl-client-bevy`.
//!
//! The pieces (added over the course of the viewer road map) are:
//!
//! - [`lod`] — the [`PrimLod`] level newtype and its detail→step-count map.
//! - [`shape`] — the dequantized float [`PrimShape`] tessellation input and its
//!   [`PathCurve`] / [`ProfileCurve`] / [`HoleType`] curve enums.
//! - [`geometry`] — the [`PrimMesh`] / [`PrimFace`] output types.
//! - [`profile`] — the 2D profile ring (square / circle / half-circle /
//!   triangles), cut and hollow, plus its semantic face ranges.
//! - [`path`] — the extrusion path (line / circle / circle2) with twist, taper,
//!   shear, skew, radius offset, and revolutions.
//! - [`volume`] — the profile-along-path sweep into per-face geometry, the join
//!   of [`profile`] and [`path`] ([`tessellate`]).
//!
//! Phase 3.1 lands the types (LOD, shape, geometry containers), Phase 3.2 the
//! [`profile`] ring, Phase 3.3 the [`path`], and Phase 3.4 the [`volume`] sweep.

pub mod geometry;
pub mod lod;
pub mod path;
pub mod profile;
pub mod shape;
pub mod volume;

pub use geometry::{PrimFace, PrimFaceId, PrimMesh};
pub use lod::{MIN_DETAIL_FACES, PRIM_LOD_COUNT, PrimLod};
pub use path::{Path, PathPoint};
pub use profile::{Profile, ProfileFace, ProfileFaceId, ProfilePoint};
pub use shape::{HoleType, PathCurve, PrimShape, ProfileCurve};
pub use volume::tessellate;
