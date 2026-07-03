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
//! - `profile` (later) — the 2D profile ring (square / circle / half-circle /
//!   triangles), cut and hollow.
//! - `path` (later) — the extrusion path (line / circle / circle2) with twist,
//!   taper, shear, skew, radius offset, and revolutions.
//! - `volume` (later) — the profile-along-path sweep into per-face geometry.
//!
//! Phase 3.1 lands the types (LOD, shape, geometry containers); the
//! tessellation itself lands in the later phases.

pub mod geometry;
pub mod lod;
pub mod shape;

pub use geometry::{PrimFace, PrimFaceId, PrimMesh};
pub use lod::{MIN_DETAIL_FACES, PRIM_LOD_COUNT, PrimLod};
pub use shape::{HoleType, PathCurve, PrimShape, ProfileCurve};
