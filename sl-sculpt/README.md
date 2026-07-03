# sl-sculpt

Pure **sculpt-texture** tessellation for Second Life / OpenSim clients: a
decoded RGB sculpt map is read as a displacement grid and stitched into
geometry. It is the sculpt counterpart of `sl-prim` (parametric prims) and
`sl-mesh` (LLMesh), and reuses `sl-prim`'s `PrimMesh` / `PrimFace` output type.

Like its siblings the crate is **Bevy-free and I/O-free**, producing geometry
in Second Life's right-handed **Z-up** space; the `to_bevy_prim_mesh`
conversion lives in `sl-client-bevy`.

A sculpt map's pixel `(r, g, b) / 255 - 0.5` becomes a grid vertex; the map is
resampled to a fixed working size and stitched per sculpt type — plane (no
wrap), cylinder (wrap U), sphere (wrap U + collapsed poles), or torus (wrap U +
V) — honouring the mirror / invert flags. A degenerate map falls back to a
placeholder grid rather than panicking.

The tessellation follows Firestorm's `LLVolume::sculpt` /
`sculptGenerateMapVertices`, reimplemented idiomatically rather than copied.

This is a Phase 0 scaffold: the stitching itself lands in a later phase.
