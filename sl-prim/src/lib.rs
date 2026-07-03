//! Pure Linden prim tessellation for Second Life / OpenSim clients — the
//! geometry counterpart of `sl-mesh` and `sl-texture`.
//!
//! See the crate `README.md` for an overview. Given a prim's shape parameters
//! this crate sweeps a 2D profile ring along an extrusion path and assembles
//! per-face geometry (positions, normals, texture coordinates, indices) in
//! Second Life's right-handed **Z-up** space. It is deliberately Bevy-free and
//! I/O-free; the `to_bevy_prim_mesh` conversion lives in `sl-client-bevy`.
//!
//! The pieces (added over the course of the viewer road map) will be:
//!
//! - `profile` — the 2D profile ring (square / circle / half-circle /
//!   triangles), cut and hollow.
//! - `path` — the extrusion path (line / circle / circle2) with twist, taper,
//!   shear, skew, radius offset, and revolutions.
//! - `volume` — the profile-along-path sweep into per-face geometry.
//!
//! This is a Phase 0 scaffold: the tessellation itself lands in later phases.
