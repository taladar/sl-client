# sl-sculpt

Pure **sculpt-texture** tessellation for Second Life / OpenSim clients: a
decoded RGB sculpt map is read as a displacement grid and stitched into
geometry. It is the sculpt counterpart of `sl-prim` (parametric prims) and
`sl-mesh` (LLMesh), and reuses `sl-prim`'s `PrimMesh` / `PrimFace` output type.

Like its siblings the crate is **Bevy-free and I/O-free**, producing geometry
in Second Life's right-handed **Z-up** space; the `to_bevy_prim_mesh`
conversion lives in `sl-client-bevy`.

A sculpt map's pixel `(r, g, b) / 255 - 0.5` becomes a grid vertex; the map is
resampled onto a working grid and stitched per sculpt type — plane (no wrap),
cylinder (wrap U), sphere (wrap U + collapsed poles), or torus (wrap U + V) —
honouring the mirror / invert flags. A degenerate map falls back to a
placeholder grid rather than panicking.

The grid is sized by `mesh_resolution` from the map's dimensions and the
requested `sl_prim::PrimLod`, matching the reference's
`sculpt_calc_mesh_resolution`: the level of detail caps the vertex budget, the
map caps it again (a vertex per four pixels), and what is left is split between
the axes in the map's own aspect ratio. So a distant sculpt is not tessellated
at full rez, and a small map is not resampled past what it carries.

The tessellation follows Firestorm's `LLVolume::sculpt` /
`sculptGenerateMapVertices`, reimplemented idiomatically rather than copied.

## Usage

`tessellate(map, sculpt_type, lod)` (or `tessellate_with(map, params, lod)` when
the `sculpt_type` byte is already parsed) takes an `sl_texture::DecodedImage`
and returns a single-face `sl_prim::PrimMesh`. Seam and pole vertices are
*shared* (one vertex referenced by the surrounding quads), never duplicated, so
the per-vertex normals accumulated from the incident triangles are smooth across
them. The caller sources the decoded map from the shared `sl-texture`
`TextureStore` — this crate never fetches or decodes.
