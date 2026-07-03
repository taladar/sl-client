//! Pure sculpt-texture tessellation for Second Life / OpenSim clients — the
//! sculpt counterpart of `sl-prim` and `sl-mesh`.
//!
//! See the crate `README.md` for an overview. A decoded RGB sculpt map is read
//! as a displacement grid and stitched into geometry (reusing `sl-prim`'s
//! `PrimMesh` / `PrimFace` output type) in Second Life's right-handed **Z-up**
//! space. It is deliberately Bevy-free and I/O-free; the `to_bevy_prim_mesh`
//! conversion lives in `sl-client-bevy`.
//!
//! Stitching honours the four sculpt types (plane / cylinder / sphere / torus)
//! and the mirror / invert flags, following Firestorm's `LLVolume::sculpt` /
//! `sculptGenerateMapVertices`, reimplemented idiomatically.
//!
//! This is a Phase 0 scaffold: the stitching itself lands in a later phase.
