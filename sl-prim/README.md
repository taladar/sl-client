# sl-prim

Pure Linden **prim tessellation** for Second Life / OpenSim clients — the
geometry counterpart of `sl-mesh` (LLMesh decode) and `sl-sculpt` (sculpt
maps). Given a prim's shape parameters it sweeps a 2D **profile** ring along an
extrusion **path** and assembles per-face vertices, normals, texture
coordinates, and indices.

This crate is deliberately **Bevy-free and I/O-free**, mirroring `sl-mesh` /
`sl-texture`: it produces a plain `PrimMesh` of `PrimFace` values in Second
Life's right-handed **Z-up** coordinate space. The `to_bevy_prim_mesh`
conversion (and the Y-up flip) lives in `sl-client-bevy`, at the entity
boundary.

The tessellation follows Firestorm's `indra/llmath/llvolume.cpp`
(`LLProfile` / `LLPath` / `LLVolume` / `LLVolumeFace`), reimplemented
idiomatically rather than copied — with a fixed high level of detail (no
distance-based LOD switching).

Sculpt-texture prims are handled by the sibling `sl-sculpt` crate, which reuses
this crate's `PrimMesh` / `PrimFace` output type.
